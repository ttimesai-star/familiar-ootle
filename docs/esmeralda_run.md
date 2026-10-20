# Familiar on Esmeralda

Run at epoch 12526. Testnet only, tTARI has no value. All keys were generated for this run and discarded; only transaction ids are published.

Policy: limit 1000000 uT per 10000 epochs, at most 400000 uT per single spend (native covenant).

**Setup and payments**

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 1. faucet to one-time fee coins | [9a45af08835d...](https://explorer.tari.mw/tx/9a45af08835ddf2fe94f4ad9bfb87fb8ab834691a3273d73dd45581053f45b7e) | Commit | agent, owner, stranger keys are fresh, not accounts |
| 2. publish template | [a688733a55cd...](https://explorer.tari.mw/tx/a688733a55cd9dcc4d195ec794b45108eb9518d0862128553df5c610b635f734) | Commit | 182084 bytes, fee 1826067 uT (dry run 1739111) |
| 3. create leash | [051a7ce97af2...](https://explorer.tari.mw/tx/051a7ce97af2792c9820760fe069af6d06280ac97510499c41920414dfac6244) | Commit | template `fe0fd2e1762f9ba5689ce8f87d16d9dd4290b77322f038af49e378522a3b4304`; two allow-listed payee roots |
| 3b. set budget root + per-spend cap (owner) | [2f12948f1627...](https://explorer.tari.mw/tx/2f12948f1627b79a779e652a9208cc1347d5819d0a7b946ae783d97b28014f2f) | Commit | leash `component_a5499480d833867dae5180143b1c2391c468dadf7684d6158bb6896f979d5d91`; cap 400000 uT, the same number as in the errand leaf |
| 4. fund budget (2 hidden UTXOs) | [bf9a26affcfe...](https://explorer.tari.mw/tx/bf9a26affcfe7bba7b47dbcd120d2fe3f78f39db91ab369640643370836159f9) | Commit | amounts are Pedersen commitments: the budget is not visible |
| 5a. try one spend over the per-spend cap | [fe1baf6c9cd3...](https://explorer.tari.mw/tx/fe1baf6c9cd38346749f0984e69ace2666d2abae4f0039fb6649665dcfa05327) | FeeIntentCommit - refused: `At instruction #1: Spend condition not met: covenant not satisfied: BalancePreserved(400000)` | refused by the covenant inside the UTXO |
| 5. agent pays payee 1 400000 uT | [b948d150fe5d...](https://explorer.tari.mw/tx/b948d150fe5db9cdac367832976e3a9dca88b7e88866b584ea5b9796530e719f) | Commit | the payee is a stealth output under an opaque root |
| 5. agent pays payee 2 400000 uT | [d9d702350372...](https://explorer.tari.mw/tx/d9d7023503722ac42391436456e0139a8dda43b06fd0bfb9e408df2dee177069) | Commit | the payee is a stealth output under an opaque root |

**Try to break it** (each transaction was submitted; the refusal is the network's)

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 6b. over the window limit | [da7a40880e92...](https://explorer.tari.mw/tx/da7a40880e9291d29d443d01987e8c27c6f3ac88105b35f2301988cc899aacf9) | FeeIntentCommit - refused: `At instruction #1: Template error: familiar: window limit exceeded: 800000 of 1000000 spent, 200000 left, payment of 300000 refused` | refused by the leash component |
| 6c. pay itself instead of a payee | [3f3c6e29295e...](https://explorer.tari.mw/tx/3f3c6e29295e31f2211eb436f920eed026abf3ec5cdb38a84d46ea5e2b59daed) | FeeIntentCommit - refused: `At instruction #1: Template error: familiar: output to a payee not on the leash` | refused by the leash component |
| 6d. spend the budget outside the leash | [0655cd78d3c3...](https://explorer.tari.mw/tx/0655cd78d3c340b1adb51fcf42df57f0bc1e43948dcaf377f1c22afdeeb9f05d) | FeeIntentCommit - refused: `At instruction #1: No active call frame` | refused by the engine: the errand leaf needs the leash frame |
| 6e. a stranger calls pay | [1b8f9c3655ea...](https://explorer.tari.mw/tx/1b8f9c3655ea61c6c29237a6058af9086c2645aa8e194c5ffdce52597581daae) | FeeIntentCommit - refused: `At instruction #1: Access Denied: call component method 'pay' on component_a5499480d833867dae5180143b1c2391c468dadf7684d6158bb6896f979d5d91` | refused: pay requires the agent's one-time key |

**After the attacks**

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 7. agent pays the last 200 000 uT of the window | [cff3b1faf3a2...](https://explorer.tari.mw/tx/cff3b1faf3a2c46628018fc5ceb47701a0b803819070600e1543bb01212ed3a4) | Commit | window: 1 000 000 of 1 000 000 |
| 8. owner recalls the rest | [341de1fc9db1...](https://explorer.tari.mw/tx/341de1fc9db111a4a678e9332295a9babb0838e82668fc18ff887115fccc0044) | Commit | the recall leaf was hidden until this moment |

Template: `fe0fd2e1762f9ba5689ce8f87d16d9dd4290b77322f038af49e378522a3b4304`. Leash: `component_a5499480d833867dae5180143b1c2391c468dadf7684d6158bb6896f979d5d91`. Budget root: `5c474f3c5bea31a9513f08121b01d1dce698632868579f759b154f8372b40af8`. Payee roots: `4128d5515f58c643649b3f4130aa5c05e1a05de96ef7471fd2c31d09f09be6f5`, `50dc64c686d6b7d994c0ea61dc8e05567d8bc4a7995e60c1d88124cbc7cc8b18`.
