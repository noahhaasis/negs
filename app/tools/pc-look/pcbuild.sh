#!/bin/bash
# Fetches the Mac branch into the PC worktree and builds app/ there.
ssh pc "C:\\Users\\Noah\\.negs\\wt.cmd gpui-look gpui-look" 2>/dev/null
ssh pc "cd /d C:\\Users\\Noah\\Projects\\negs\\worktrees\\gpui-look && set \"PATH=C:\\Users\\Noah\\.cargo\\bin;%PATH%\"&& set CARGO_PROFILE_RELEASE_LTO=false&& set CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16&& set CARGO_PROFILE_RELEASE_INCREMENTAL=true&& cargo build --release --manifest-path app\\Cargo.toml 2>&1" 2>/dev/null | grep -E '^error|^warning: unused|-->|Finished' | grep -v white_balance
