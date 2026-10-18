#!/usr/bin/env bash
# Esmeralda testnet only: get tXTM on L1 from the public faucet, burn it to the Ootle funder's account public key and
# leave the burn proof for `familiar claim`. Inputs: claim_pk.hex (64 hex chars). Writes BURN_AMOUNT and PROOF_FILE to
# $GITHUB_ENV. The L1 wallet is created for this run only; its password is masked and never printed.
set -euo pipefail

SUITE=https://github.com/tari-project/tari/releases/download/v6.1.0/tari_suite-6.1.0-esme-463e53d-linux-x86_64.zip
PROTO_BASE=https://raw.githubusercontent.com/tari-project/tari/v6.1.0/applications/minotari_app_grpc/proto
GRPCURL=https://github.com/fullstorydev/grpcurl/releases/download/v1.9.4/grpcurl_1.9.4_linux_x86_64.tar.gz

mkdir -p l1/bin l1/proto l1/burn_proofs
curl -sSL -o l1/suite.zip "$SUITE"
unzip -o -q l1/suite.zip -d l1/suite
W=$(find l1/suite -name minotari_console_wallet -type f | head -1)
chmod +x "$W"
curl -sSL "$GRPCURL" | tar xz -C l1/bin grpcurl
for f in base_node block network p2pool sidechain_types transaction types wallet; do
  curl -sSL -o "l1/proto/$f.proto" "$PROTO_BASE/$f.proto"
done

MINOTARI_WALLET_PASSWORD=$(openssl rand -hex 16)
echo "::add-mask::$MINOTARI_WALLET_PASSWORD"
export MINOTARI_WALLET_PASSWORD
BASE=$PWD/l1/wallet
"$W" --base-path "$BASE" --network esmeralda --non-interactive-mode --grpc-enabled \
  --grpc-address /ip4/127.0.0.1/tcp/18143 --burn-proof-out "$PWD/l1/burn_proofs" > l1/wallet.out 2>&1 &
echo "wallet pid $!"

G() { l1/bin/grpcurl -plaintext -import-path l1/proto -proto wallet.proto -d "$2" 127.0.0.1:18143 "tari.rpc.Wallet/$1"; }

for i in $(seq 1 60); do
  if G GetCompleteAddress '{}' > l1/address.json 2>/dev/null; then break; fi
  sleep 5
done
ADDR=$(jq -r .oneSidedAddressBase58 l1/address.json)
echo "L1 one-sided address: $ADDR"

echo "requesting tXTM from the faucet"
curl -sS -X POST https://testnet-faucet.supportxtm.com/request \
  --data-urlencode "address=$ADDR" --data-urlencode "website=" \
  | sed -e 's/<[^>]*>/ /g' | tr -s ' \n' | grep -v '^ *$' | grep -iv -- '--jag\|{\|}\|;' | head -20

AVAIL=0
for i in $(seq 1 120); do
  B=$(G GetBalance '{}' 2>/dev/null || echo '{}')
  AVAIL=$(echo "$B" | jq -r '.availableBalance // "0"')
  PEND=$(echo "$B" | jq -r '.pendingIncomingBalance // "0"')
  echo "$(date -u +%H:%M:%S) available=$AVAIL pending=$PEND"
  if [ "$AVAIL" -gt 0 ]; then break; fi
  sleep 30
done
[ "$AVAIL" -gt 0 ] || { echo "no tXTM arrived"; grep -Eiv "seed|word|mnemonic" l1/wallet.out | tail -40; exit 1; }

# keep a margin for the L1 fee
AMOUNT=$((AVAIL - 200000))
CLAIM_PK_B64=$(xxd -r -p claim_pk.hex | base64 -w0)
G CreateBurnTransaction "{\"amount\": $AMOUNT, \"fee_per_gram\": 5, \"claim_public_key\": \"$CLAIM_PK_B64\"}" > l1/burn.json
jq '{transactionId, isSuccess, failureMessage, commitment}' l1/burn.json
[ "$(jq -r .isSuccess l1/burn.json)" = "true" ] || exit 1

PROOF=""
for i in $(seq 1 180); do
  PROOF=$(find l1/burn_proofs -maxdepth 1 -name '*.json' | head -1)
  if [ -n "$PROOF" ]; then break; fi
  sleep 30
done
[ -n "$PROOF" ] || { echo "no burn proof written"; grep -Eiv "seed|word|mnemonic" l1/wallet.out | tail -40; exit 1; }
echo "proof file: $PROOF (top-level keys: $(jq -c 'keys' "$PROOF"))"
echo "BURN_AMOUNT=$AMOUNT" >> "$GITHUB_ENV"
echo "PROOF_FILE=$PWD/$PROOF" >> "$GITHUB_ENV"
