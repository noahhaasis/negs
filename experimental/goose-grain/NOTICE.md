This experiment is covered by the repository's GPL-3.0 license.

`grain.goose`, `blur.goose` and the distribution routines in `rng.goose`
are faithful translations of this repository's Rust model, samplers and blur.
The original distribution algorithms are NumPy legacy RandomState algorithms;
the translation preserves the existing Rust implementation and its operation order.

The MT19937 implementation follows rand_mt 5.0.0 (`src/mt.rs`):

Copyright (c) 2015,2017 rust-mersenne-twister developers
Copyright (c) 2020 Ryan Lopopolo <rjl@hyperbo.la>

Used under its MIT license, reproduced in `licenses/rand_mt-MIT.txt`.

Both native executables use the exact `libm` 0.2.16 `lgamma` implementation.
The Goose driver links that scalar primitive through `src/lib.rs`, since
Goose's standard math module does not expose it. The full libm license is in
`licenses/libm.txt`; its lgamma implementation carries the following notice:

Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
Developed at SunSoft, a Sun Microsystems, Inc. business.
Permission to use, copy, modify, and distribute this software is freely granted,
provided that this notice is preserved.

No grain, random-number generation, filtering or worker operation calls Rust
from Goose. `lgamma` is a direct scalar native math call, just as the other
libm calls are; there is no Rust/Goose runtime bridge or language dispatcher.
The other shared native helper reports retained RSS outside timing.
`support.c` performs SHA-256 verification and binary64 loading outside timing.
