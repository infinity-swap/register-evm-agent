#! /bin/bash

echo "Building artifacts if not available"
if [[ ! -f .artifact/register_minter ]]; then
    ./scripts/build.sh
fi

echo "Spinning up dfx and deploy canisters"
./scripts/dfx/deploy_local.sh create &>/dev/null

# Test minter registration
identity="$HOME/.config/dfx/identity/alice/identity.pem"
evmc=$(dfx canister id evmc)
minter_canister=$(dfx canister id minter_canister)
./.artifact/register_minter register $identity  $evmc $minter_canister
status_code=$?
if [ $status_code -eq 0 ]; then
    echo "Minter registration test: passed"
else
    echo "Minter registration test: failed"
fi
