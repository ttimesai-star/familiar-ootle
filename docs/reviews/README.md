# External reviews

Unedited reports of the independent reviews, in the order they happened. Jules (Google) reviewed read-only snapshots of
this repository; its report text is taken from the file it wrote. The Gemini review is a second model of another
vendor.

| Date | Reviewer | Snapshot | Result | Report |
|---|---|---|---|---|
| 19.10 | Jules | 19.10 snapshot | 2 medium (budget-root rotation, window shrink) and lows | [jules_review_2026-10-19.md](jules_review_2026-10-19.md) |
| 19.10 | Gemini | 19.10 snapshot | same two mediums, plus notes | [gemini_review_2026-10-19.md](gemini_review_2026-10-19.md) |
| 20.10 | Jules | fixes of 19.10 | both fixes approved (session then ended with an error, text as written) | [jules_rereview_2026-10-20.md](jules_rereview_2026-10-20.md) |
| 21.10 | Jules | privacy changes (5652037) | 1 medium, 3 low | [jules_review_2026-10-21.md](jules_review_2026-10-21.md) |
| 21.10 | Jules | fixes (051061c) | approved | [jules_rereview_2026-10-21.md](jules_rereview_2026-10-21.md) |
| 21.10 | Jules | coupon mode, try-it CLI, tap, MCP (ff12a75) | no findings; one statement in it was wrong (`.gitignore` did not list the wallet file `me.json`): fixed in the commit that added this report | [jules_review_2026-10-21b.md](jules_review_2026-10-21b.md) |
| 21.10 | Jules | cycle 3: recall key of its own, owner-drawn coupon denomination, tap-status (3030e8f) | no findings; checked by the author: the recall key's absence before the recall is also confirmed on-chain by `observer_view.py` (run 39650330915). Not raised by the review, added by the author: the client prints the denomination to its own terminal (`leash`, `ask`, `show`), so the public try-it CI log shows that throwaway leash's denomination; on-chain it appears nowhere. One wording in it is loose: `tap_status` reads the indexer's JSON, not CBOR | [jules_review_2026-10-21c.md](jules_review_2026-10-21c.md) |
