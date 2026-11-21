--------------------------- MODULE FramePipeline ---------------------------
(***************************************************************************)
(* The macOS canvas's frame pipeline (EditorModel.swift): opening a        *)
(* photograph, its early frames (restored, kept, placeholder), the render  *)
(* queue with its decode, quick and settled frames, slider drags, the      *)
(* brightness match that a render waits for, and the frame held for it.   *)
(*                                                                         *)
(* The whole model state is one record `s`, and each action is written as *)
(* a chain of functions on it, so the Swift code's sequential steps        *)
(* (`changed()` then `reconcileBrightnessMatch()`, a render's `defer`)     *)
(* read the same here. Every `await` in the Swift code is a separate       *)
(* action, so anything the user or another task does can land in between. *)
(*                                                                         *)
(* Abstracted: pixels, sizes and the helper's output are left out. Where   *)
(* the code decides from them (a quick frame pays, the canvas has a scale, *)
(* a solve moves the exposure, a request fails), the model chooses freely, *)
(* so it covers every outcome. Edits are counted, not stored: `rev` is the *)
(* revision, `inputsVer` the version of what the match depends on.         *)
(***************************************************************************)
EXTENDS Integers, TLC

CONSTANTS MaxGen,    \* photographs opened
          MaxEdits   \* user actions other than opening

NoFrame == [rev |-> -1, gen |-> -1]
NoImage == [kind |-> "none", rev |-> -1, gen |-> -1]
NoHeld  == [rev |-> -1, gen |-> -1, img |-> NoImage]
RIdle   == [pc |-> "idle", rev |-> -1, gen |-> -1, step |-> "none", first |-> FALSE, finished |-> FALSE]
MIdle   == [pc |-> "idle", gen |-> -1, frame |-> NoFrame, ver |-> -1]
EIdle   == [pc |-> "idle", gen |-> -1, rev |-> -1, restored |-> FALSE]

VARIABLE s

Frame(st) == [rev |-> st.rev, gen |-> st.gen]

(* brightnessMatchActive / brightnessMatchPending *)
MatchActive(st)  == st.matchOn /\ ~(st.failGen = st.gen /\ st.failedVer = st.inputsVer)
MatchPending(st) == MatchActive(st) /\ st.solvedVer # st.inputsVer

ScheduleRender(st) == [st EXCEPT !.pending = TRUE]

(* changed(): a rendered edit *)
Changed(st, invalidateCache) ==
    LET st1 == [st EXCEPT !.held = NoHeld, !.rev = @ + 1, !.previewCurrent = FALSE, !.failed = FALSE,
                          !.showingCached = IF invalidateCache THEN FALSE ELSE @]
    IN  IF st1.gen > 0 THEN ScheduleRender(st1) ELSE st1

(* reconcileBrightnessMatch() *)
Reconcile(st) ==
    IF /\ st.m.pc = "idle" /\ ~st.interacting /\ ~st.rendering /\ ~st.cropping /\ st.gen > 0
       /\ MatchPending(st) /\ st.matchDecodeFailed # Frame(st)
    THEN [st EXCEPT !.m = [pc |-> "start", gen |-> st.gen, frame |-> Frame(st), ver |-> st.inputsVer]]
    ELSE st

(* settleQuickFrame() *)
SettleQuick(st) ==
    IF /\ st.quickStep = Frame(st) /\ ~st.previewCurrent /\ ~st.failed /\ ~st.interacting
       /\ ~st.rendering /\ ~st.pending /\ st.m.pc = "idle" /\ st.held = NoHeld /\ st.gen > 0
    THEN ScheduleRender(st) ELSE st

(* renderAfterMatch() *)
RenderAfterMatch(st) ==
    IF /\ st.gen > 0 /\ ~st.previewCurrent /\ ~st.failed /\ ~st.rendering /\ ~st.pending
       /\ ~st.interacting /\ st.held = NoHeld
    THEN ScheduleRender(st) ELSE st

(* The `show` closure of render() *)
Show(st, img) ==
    LET current == img.rev = st.rev /\ img.kind = "settled"
    IN  [st EXCEPT !.image = img, !.ph = "none", !.interactiveShown = (img.kind = "drag"),
                   !.previewCurrent = IF current THEN TRUE ELSE @,
                   !.showingCached = IF current THEN FALSE ELSE @]

(* showHeldPreview() *)
ShowHeld(st) ==
    IF st.held = NoHeld THEN st
    ELSE LET st1 == [st EXCEPT !.held = NoHeld]
         IN  IF st.held.rev = st.rev /\ st.held.gen = st.gen
             THEN SettleQuick(Show(st1, st.held.img)) ELSE st1

(* shouldShow(_:_:) *)
ShouldShow(st, rev, gen) == st.gen = gen /\ (st.rev = rev \/ st.interacting)

(* The defer at the end of render() *)
RenderDefer(st) ==
    LET r   == st.r
        st1 == [st EXCEPT !.rendering = FALSE, !.r = RIdle,
                  !.quickStep = IF st.rev = r.rev /\ st.gen = r.gen
                                THEN IF r.step = "quick" THEN [rev |-> r.rev, gen |-> r.gen]
                                     ELSE IF r.step = "settled" THEN NoFrame ELSE @
                                ELSE @]
        st2 == IF r.step = "drag" /\ ~st.interacting THEN ScheduleRender(st1) ELSE st1
        st3 == IF st.rev # r.rev THEN ScheduleRender(st2) ELSE st2
    IN  SettleQuick(Reconcile(st3))

(* render(), from choosing the step to sending the request. `q` stands for *)
(* what the model leaves out: a large enough photograph, a canvas scale    *)
(* below 1, no reusable helper output.                                     *)
BeginRequest(st, first, finished, q) ==
    LET frame       == Frame(st)
        phShown     == st.phFor.gen = st.gen /\ ~st.phFor.kept
        scale       == ~first \/ phShown
        sized       == ~first \/ st.decodedFor = st.gen
        step        == IF st.interacting THEN (IF q /\ scale THEN "drag" ELSE "settled")
                       ELSE IF q /\ scale /\ sized /\ ~finished /\ st.quickStep # frame THEN "quick"
                       ELSE "settled"
        \* ticket 39: a settled render of an edit whose match is stale
        stale       == /\ step = "settled" /\ ~st.interacting /\ ~st.cropping
                       /\ MatchPending(st) /\ st.matchDecodeFailed # frame
    IN  [st EXCEPT !.rendering = TRUE,
                   !.r = [pc |-> "requesting", rev |-> st.rev, gen |-> st.gen, step |-> step,
                          first |-> first, finished |-> finished],
                   !.sentStaleSettled = @ \/ stale]

(* The render task runs: render() up to its first await. *)
RunRender(st, q) ==
    LET st0   == [st EXCEPT !.pending = FALSE]
        frame == Frame(st0)
    IN
    IF st0.rendering THEN st0
    \* FIX 4: the current edit's settled frame shows already; a render
    \* scheduled before it landed (a release) would only replace it. Every
    \* edit clears `previewCurrent`, so nothing else needs it.
    ELSE IF st0.previewCurrent THEN st0
    ELSE IF /\ st0.quickStep = frame /\ ~st0.previewCurrent /\ ~st0.interacting
            /\ (st0.m.pc # "idle" \/ st0.held # NoHeld) THEN st0
    ELSE
    LET solveFirst == ~st0.interacting /\ ~st0.cropping /\ MatchPending(st0) /\ st0.matchDecodeFailed # frame
        st1        == IF solveFirst THEN Reconcile(st0) ELSE st0
    IN
    IF solveFirst /\ st1.m.pc # "idle" THEN st1
    ELSE
    LET first    == st1.image.kind = "none"
        finished == st1.showingCached \/ (first /\ st1.phFor.gen = st1.gen /\ st1.phFor.kept)
    IN
    IF first /\ ~finished /\ ~st1.interacting /\ st1.decodedFor # st1.gen
    THEN [st1 EXCEPT !.rendering = TRUE,
                     !.r = [pc |-> "decoding", rev |-> st1.rev, gen |-> st1.gen, step |-> "none",
                            first |-> first, finished |-> finished]]
    ELSE BeginRequest(st1, first, finished, q)

-----------------------------------------------------------------------------
(* User actions *)

Edits(st) == st.edits < MaxEdits
Spend(st) == [st EXCEPT !.edits = @ + 1]

(* A rendered edit, during a drag or not; `input` if the match depends on it. *)
Edit(input) ==
    /\ s.gen > 0 /\ Edits(s)
    /\ s' = Reconcile(Changed([Spend(s) EXCEPT !.inputsVer = IF input THEN @ + 1 ELSE @], TRUE))

(* Match camera brightness on (Exposure back to 0: a rendered change unless *)
(* it was 0 already) or off (the brightness stays).                         *)
MatchOn(moves) ==
    /\ s.gen > 0 /\ Edits(s) /\ ~s.matchOn /\ ~s.interacting
    /\ LET st1 == [Spend(s) EXCEPT !.matchOn = TRUE, !.solvedVer = -1]
       IN  s' = Reconcile(IF moves THEN Changed(st1, TRUE) ELSE st1)
MatchOff ==
    /\ s.gen > 0 /\ Edits(s) /\ s.matchOn /\ ~s.interacting
    /\ s' = [Spend(s) EXCEPT !.matchOn = FALSE]

BeginInteraction ==
    /\ s.gen > 0 /\ Edits(s) /\ ~s.interacting
    /\ s' = [Spend(s) EXCEPT !.interacting = TRUE]

(* endInteraction() *)
EndInteraction ==
    /\ s.interacting
    /\ LET st1 == [s EXCEPT !.interacting = FALSE, !.quickStep = Frame(s)]
           st2 == IF s.interactiveShown THEN ScheduleRender(st1) ELSE st1
           \* FIX 3: what waited for the release (a match to solve, the
           \* render after one) goes on now.
           st3 == Reconcile(st2)
       IN  s' = IF st3.m.pc = "idle" THEN RenderAfterMatch(SettleQuick(st3)) ELSE st3

BeginCrop ==
    /\ s.gen > 0 /\ Edits(s) /\ ~s.cropping /\ s.image.kind # "none"
    /\ s' = [Spend(s) EXCEPT !.cropping = TRUE]

(* endCrop(): Done replaces the state (the crop only redraws the canvas),  *)
(* which reconciles; Cancel replaces it only if it turned the photograph.  *)
EndCrop ==
    /\ s.cropping
    \* FIX 6: Done or Cancel, what waited for the crop goes on (as FIX 3
    \* for a release). Before it, Cancel left this state unchanged.
    /\ LET st1 == Reconcile([s EXCEPT !.cropping = FALSE])
       IN  s' = IF st1.m.pc = "idle" THEN RenderAfterMatch(st1) ELSE st1

(* retryDevelop() *)
Retry ==
    /\ s.failed /\ Edits(s)
    /\ s' = Changed(Spend(s), FALSE)

(* open(_:restoring:): `restored` with a session (its saved frame may be   *)
(* read); the photograph's own edits may match brightness, solved or not.  *)
Open(restored, matchOn, solved, standIn) ==
    /\ s.gen < MaxGen
    /\ LET g   == s.gen + 1
           v   == s.inputsVer + 1
           st1 == [s EXCEPT !.m = MIdle,
                     !.image = NoImage, !.ph = IF ~restored /\ standIn THEN "standin" ELSE "none",
                     !.previewCurrent = FALSE, !.failed = FALSE, !.showingCached = FALSE,
                     !.gen = g, !.interacting = FALSE, !.cropping = FALSE,
                     !.matchOn = matchOn, !.inputsVer = v, !.solvedVer = IF solved THEN v ELSE -1]
           st2 == Changed(st1, ~restored)
       IN  s' = [st2 EXCEPT !.early = [pc |-> "start", gen |-> g, rev |-> st2.rev, restored |-> restored]]

-----------------------------------------------------------------------------
(* Tasks: each step is what runs between two awaits. *)

RenderTask ==
    /\ s.pending
    /\ \E q \in BOOLEAN : s' = RunRender(s, q)

(* The decode before a first frame. A failed one leaves its error to the render. *)
DecodeDone ==
    /\ s.r.pc = "decoding"
    /\ LET r   == s.r
           st1 == [s EXCEPT !.decodedFor = IF r.gen = s.gen THEN r.gen ELSE @, !.rendering = FALSE, !.r = RIdle]
           \* FIX 1: matching turned on during the decode is solved first too.
           solveFirst == /\ ~st1.interacting /\ ~st1.cropping /\ MatchPending(st1)
                         /\ st1.matchDecodeFailed # Frame(st1)
           st2 == IF solveFirst THEN Reconcile(st1) ELSE st1
       IN  IF s.rev # r.rev \/ s.gen # r.gen
           THEN s' = ScheduleRender(st1)
           ELSE IF solveFirst /\ st2.m.pc # "idle" THEN s' = st2
           ELSE \E q \in BOOLEAN : s' = BeginRequest(st2, r.first, r.finished, q)

Showable(st) ==
    /\ IF st.r.step = "quick" THEN st.gen = st.r.gen ELSE ShouldShow(st, st.r.rev, st.r.gen)
    \* FIX 5: a canvas-size frame never replaces the current edit's settled one.
    /\ st.r.step = "settled" \/ ~st.previewCurrent

(* A failed render: a quick frame leaves the error to the settled one. *)
RenderFailed(st) ==
    LET r == st.r
    IN  RenderDefer(IF r.step # "quick" /\ st.rev = r.rev /\ st.gen = r.gen
                    THEN [st EXCEPT !.failed = TRUE] ELSE st)

RequestDone ==
    /\ s.r.pc = "requesting"
    /\ \/ s' = RenderFailed(s)
       \/ s' = IF Showable(s) THEN [s EXCEPT !.r.pc = "reading"] ELSE RenderDefer(s)

(* The frame is read: shown, or held for a match that went stale meanwhile. *)
ReadDone ==
    /\ s.r.pc = "reading"
    /\ \/ s' = RenderFailed(s)
       \/ s' = IF ~Showable(s) THEN RenderDefer(s)
               ELSE LET r   == s.r
                        img == [kind |-> r.step, rev |-> r.rev, gen |-> r.gen]
                        \* FIX 2: not for an edit whose match decode failed: no solve
                        \* would ever release it.
                        hold == /\ r.step # "drag" /\ s.rev = r.rev /\ ~s.interacting /\ ~s.cropping
                                /\ MatchPending(s) /\ s.matchDecodeFailed # Frame(s)
                    IN  RenderDefer(IF hold THEN [s EXCEPT !.held = [rev |-> r.rev, gen |-> r.gen, img |-> img]]
                                    ELSE Show(s, img))

(* The match task's defer *)
MatchFinish(st) ==
    LET st1 == Reconcile([st EXCEPT !.m = MIdle])
    IN  IF st1.m.pc = "idle" THEN RenderAfterMatch(SettleQuick(ShowHeld(st1))) ELSE st1

(* applyBrightnessMatch(): kept if the inputs are still those it solved;   *)
(* `moves` when the exposure moves by 0.05 EV or more (a rendered change). *)
ApplyMatch(st, moves) ==
    IF st.inputsVer = st.m.ver /\ st.matchOn
    THEN LET st1 == [st EXCEPT !.solvedVer = st.inputsVer]
         IN  IF moves THEN Reconcile(Changed(st1, TRUE)) ELSE st1
    ELSE st

Wanted(st) == st.gen = st.m.gen

MatchStart ==
    /\ s.m.pc = "start"
    /\ \/ \E moves \in BOOLEAN :   \* solved before: from the cache
            s' = MatchFinish(IF Wanted(s) THEN ApplyMatch(s, moves) ELSE s)
       \/ s' = [s EXCEPT !.m.pc = "decoding"]

MatchDecodeDone ==
    /\ s.m.pc = "decoding"
    /\ \/ s' = MatchFinish([s EXCEPT !.matchDecodeFailed = s.m.frame])
       \/ s' = [s EXCEPT !.decodedFor = s.m.gen, !.m.pc = "solving"]

MatchSolveDone ==
    /\ s.m.pc = "solving"
    /\ \/ \E moves \in BOOLEAN : s' = MatchFinish(ApplyMatch(s, moves))
       \/ s' = MatchFinish(IF s.inputsVer = s.m.ver
                           THEN [s EXCEPT !.failGen = s.gen, !.failedVer = s.inputsVer] ELSE s)

(* The early frames of open(): the restored session's saved frame, else    *)
(* the kept last develop, else the camera's thumbnail.                     *)
EarlyCached ==
    /\ s.early.pc = "start" /\ s.early.restored
    /\ IF s.gen = s.early.gen /\ s.image.kind = "none"
       THEN IF s.rev = s.early.rev
            THEN s' = [s EXCEPT !.early = EIdle, !.ph = "none", !.showingCached = TRUE,
                                !.image = [kind |-> "cached", rev |-> s.rev, gen |-> s.gen]]
            ELSE s' = [s EXCEPT !.early.pc = "thumbnail"]
       ELSE s' = [s EXCEPT !.early = EIdle]

EarlyThumbnail ==
    /\ \/ s.early.pc = "thumbnail"
       \/ s.early.pc = "start" /\ ~s.early.restored
    /\ \E kept \in BOOLEAN :
         s' = IF s.gen = s.early.gen /\ s.image.kind = "none"
              THEN [s EXCEPT !.early = EIdle, !.ph = IF kept THEN "kept" ELSE "thumb",
                             !.phFor = [gen |-> s.gen, kept |-> kept]]
              ELSE [s EXCEPT !.early = EIdle]

-----------------------------------------------------------------------------

Init ==
    s = [gen |-> 0, rev |-> 0, edits |-> 0,
         interacting |-> FALSE, cropping |-> FALSE,
         matchOn |-> FALSE, inputsVer |-> 0, solvedVer |-> -1, failGen |-> -1, failedVer |-> -1,
         image |-> NoImage, ph |-> "none", phFor |-> [gen |-> -1, kept |-> FALSE],
         previewCurrent |-> FALSE, failed |-> FALSE, showingCached |-> FALSE, interactiveShown |-> FALSE,
         quickStep |-> NoFrame, decodedFor |-> -1, matchDecodeFailed |-> NoFrame, held |-> NoHeld,
         pending |-> FALSE, rendering |-> FALSE,
         r |-> RIdle, m |-> MIdle, early |-> EIdle,
         sentStaleSettled |-> FALSE]

User ==
    \/ \E input \in BOOLEAN : Edit(input)
    \/ \E moves \in BOOLEAN : MatchOn(moves)
    \/ MatchOff
    \/ BeginInteraction \/ EndInteraction
    \/ BeginCrop \/ EndCrop
    \/ Retry
    \/ \E restored, on, solved, standIn \in BOOLEAN : Open(restored, on, solved, standIn)

Tasks ==
    \/ RenderTask \/ DecodeDone \/ RequestDone \/ ReadDone
    \/ MatchStart \/ MatchDecodeDone \/ MatchSolveDone
    \/ EarlyCached \/ EarlyThumbnail

Next == User \/ Tasks

(* Every task that can run eventually runs; the user may stop at any time. *)
Fairness ==
    /\ WF_s(RenderTask) /\ WF_s(DecodeDone) /\ WF_s(RequestDone) /\ WF_s(ReadDone)
    /\ WF_s(MatchStart) /\ WF_s(MatchDecodeDone) /\ WF_s(MatchSolveDone)
    /\ WF_s(EarlyCached) /\ WF_s(EarlyThumbnail)

Spec == Init /\ [][Next]_s /\ Fairness

-----------------------------------------------------------------------------
(* Safety *)

(* The canvas never shows another photograph's frame. *)
ImageOfThisPhoto == s.image.kind # "none" => s.image.gen = s.gen

(* A developed frame is never covered by a placeholder. *)
NoPlaceholderOverFrame == s.image.kind # "none" => s.ph = "none"

(* `previewCurrent` (Save, Export, the thumbnails) means the shown frame is *)
(* the settled render of the current edit.                                  *)
PreviewCurrentIsTrue ==
    s.previewCurrent => /\ s.image.kind = "settled" /\ s.image.rev = s.rev /\ s.image.gen = s.gen

(* A held frame is this photograph's. *)
HeldIsThisPhoto == s.held # NoHeld => s.held.gen = s.gen

(* Ticket 39: the old exposure is never rendered at export size. *)
NoSettledRenderOfStaleMatch == ~s.sentStaleSettled

(* A held frame waits for a solve that is running, which shows it. *)
HeldHasSolve == s.held # NoHeld => s.m.pc # "idle"

(* Only one render at a time. *)
OneRender == s.r.pc # "idle" => s.rendering

-----------------------------------------------------------------------------
(* Liveness: once the user lets go, the canvas settles: the current edit's *)
(* settled frame (or its error) shows, and no match is left pending.       *)

Quiet ==
    \/ s.gen = 0
    \/ /\ ~s.pending /\ s.r.pc = "idle" /\ s.m.pc = "idle" /\ s.held = NoHeld
       /\ \/ s.failed
          \/ s.previewCurrent /\ ~MatchPending(s)
          \/ s.previewCurrent /\ s.matchDecodeFailed = Frame(s)

UserLetGo == ~s.interacting /\ ~s.cropping

Settles == (<>[]UserLetGo) => <>[]Quiet

=============================================================================
