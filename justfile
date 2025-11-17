# https://just.systems

generate-dump:
    node scripts/extract-grammar-metadata.js
    cargo run --release --bin=build-registry --features=tools

generate-tests:
    cd vscode-textmate && npm install && npm run compile
    node scripts/generate-scopes.js
    node scripts/generate-snapshots.js

update-submodules:
    git submodule update --init --recursive --remote
