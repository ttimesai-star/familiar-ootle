# Familiar on Esmeralda

Run at epoch 12448. Testnet only, tTARI has no value. All keys were generated for this run and discarded; only transaction ids are published.

Policy: limit 1000000 uT per 10000 epochs, at most 400000 uT per single spend (native covenant).

**Setup and payments**

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 1. faucet to one-time fee coins | [ac0423ece728...](https://explorer.tari.mw/tx/ac0423ece728d8fcc414090b21a93c45e2b72ee6c48f6cad7b8a1734be23bedb) | Commit | agent, owner, stranger keys are fresh, not accounts |
| 2. publish template | [86ef4df92a0d...](https://explorer.tari.mw/tx/86ef4df92a0d2c72d664e031495814cd213d6d8b032d55db64c7922bc1b2d814) | Commit | 181160 bytes, fee 1807177 uT (dry run 1721120) |
| 3. create leash | [2235be01aeb3...](https://explorer.tari.mw/tx/2235be01aeb3f9b71b99bd0bf323b5cc6b54498f459e869f668cb89115cb3450) | Commit | template `029b13158f34c34c18e331e7d7c0a43ac50ccbb99d6f9b62e9168fe7fb23686a`; two allow-listed payee roots |
| 3b. set budget root (owner) | [23e7daac37d3...](https://explorer.tari.mw/tx/23e7daac37d3a9e58d7836fe7827aad9a66bbd9feab8aa35a220b4bc3dc1f0d8) | Commit | leash `component_8f87e8e2a7aa367e07f4cfaccf175633db030c2b92a76f855482a156de3b7b34` |
| 4. fund budget (2 hidden UTXOs) | [43c1fa9e6770...](https://explorer.tari.mw/tx/43c1fa9e6770fe28bc7f13f8b2e403b7e427af70bae42db4a8321d74e46f5a38) | Commit | amounts are Pedersen commitments: the budget is not visible |
| 5a. try one spend over the per-spend cap | [8e99dc151577...](https://explorer.tari.mw/tx/8e99dc151577369c291ea950cf3c1483cfa24f65ab4578afe6c6d64dd40bbb8e) | FeeIntentCommit - refused: `At instruction #1: Spend condition not met: covenant not satisfied: BalancePreserved(400000)` | refused by the covenant inside the UTXO |
| 5. agent pays payee 1 400000 uT | [559c35061258...](https://explorer.tari.mw/tx/559c35061258b7e3dc2f831d6966a86c14f4f2b1b3a58f251dbd0ae92a83864c) | Commit | the payee is a stealth output under an opaque root |
| 5. agent pays payee 2 400000 uT | [54e3c7eee93e...](https://explorer.tari.mw/tx/54e3c7eee93e67aa6ede10fecaef091385628494014a69cf352ae5ec4f552913) | Commit | the payee is a stealth output under an opaque root |

**Try to break it** (each transaction was submitted; the refusal is the network's)

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 6b. over the window limit | [474668819ce4...](https://explorer.tari.mw/tx/474668819ce4a201ef7bcf5aef9b188bea8a82e3e3b9d444188cdaa29b2b0ee7) | FeeIntentCommit - refused: `At instruction #1: Template error: familiar: window limit exceeded: 800000 of 1000000 spent, 200000 left, payment of 300000 refused` | refused by the leash component |
| 6c. pay itself instead of a payee | [7da62ee4c2f1...](https://explorer.tari.mw/tx/7da62ee4c2f1729b822f87096f2fd24fd25b0a71f62c83adc37e4a7216effd33) | FeeIntentCommit - refused: `At instruction #1: Template error: familiar: output to a payee not on the leash` | refused by the leash component |
| 6d. spend the budget outside the leash | [6b594f16a682...](https://explorer.tari.mw/tx/6b594f16a6823127b5f1d84c3e66e3818eb77b3985751757ecfa4556d8e6380f) | FeeIntentCommit - refused: `At instruction #1: No active call frame` | refused by the engine: the errand leaf needs the leash frame |
| 6e. a stranger calls pay | [f4b423906561...](https://explorer.tari.mw/tx/f4b423906561ffdeb3d86275ba6cde42630f1759374f7f1134499a896859ce17) | FeeIntentCommit - refused: `At instruction #1: Access Denied: call component method 'pay' on component_8f87e8e2a7aa367e07f4cfaccf175633db030c2b92a76f855482a156de3b7b34` | refused: pay requires the agent's one-time key |

**After the attacks**

| Step | Transaction | Outcome | Note |
|---|---|---|---|
| 7. agent pays the last 200 000 uT of the window | [1fd07eaf2e4f...](https://explorer.tari.mw/tx/1fd07eaf2e4fb0783b707fb368db89de27dd64ce37b1472735d7cf7978f3fd00) | Commit | window: 1 000 000 of 1 000 000 |
| 8. owner recalls the rest | [2919381a52ef...](https://explorer.tari.mw/tx/2919381a52efda5e753da355bff1ca953988bf5eb505a312798feadd3e1d182f) | Commit | the recall leaf was hidden until this moment |

Template: `029b13158f34c34c18e331e7d7c0a43ac50ccbb99d6f9b62e9168fe7fb23686a`. Leash: `component_8f87e8e2a7aa367e07f4cfaccf175633db030c2b92a76f855482a156de3b7b34`. Budget root: `e153741efbfaf5a8a6da645599feb7a8e1b21e384d1d3268ca0a3c76559f3290`. Payee roots: `783a6b1ed85da5aee0dfa66c02d54af840837f8ac17b8c301d4e6465c4109cfc`, `ebdc32e9e11a91c87c99abc707d4b8b64fe1b959a4d234dcef59b7a8ee117e52`.
