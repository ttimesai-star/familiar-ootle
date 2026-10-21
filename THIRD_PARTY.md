# Third-party code

All code in this repository is new (written from 18 October 2026) and MIT-licensed (see `LICENSE`), with one
exception:

## `client/src/l1proof.rs` — adapted from tari-ootle (BSD-3-Clause)

The conversion of a Minotari (L1) burn proof file into the Ootle claim proof is adapted from two functions of
[tari-project/tari-ootle](https://github.com/tari-project/tari-ootle) v0.46.1:

* `claim_proof_from_l1` in `applications/tari_ootle_app_utilities/src/burn_claim_proof.rs`
  (file header: "Copyright 2026 The Tari Project, SPDX-License-Identifier: BSD-3-Clause");
* `complete_burn_proof_to_contents` in `applications/tari_walletd/src/handlers/helpers.rs`
  (file header: "Copyright 2023 The Tari Project, SPDX-License-Identifier: BSD-3-Clause").

That crate is not published on crates.io, so the conversion is reproduced here. It is used only by the
`familiar claim` command (funding a test account from an L1 burn); the template and the leash logic do not depend on
it. The tari-ootle licence, reproduced as its condition 1 requires:

```
BSD 3-Clause License

Copyright (c) 2019, The Tari Developer Community
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Dependencies

Crates from crates.io (`tari_template_lib`, `tari_template_test_tooling`, `ootle-rs`, `tari_ootle_wallet_crypto`
and others, see `Cargo.lock`) are used as libraries under their own licences; no code of theirs is copied here.
