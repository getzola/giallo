# https://just.systems

generate-dump:
    node scripts/extract-grammar-metadata.js
    cargo run --release --bin=build-registry --features=tools

generate-tests:
    cd vscode-textmate && npm install && npm run compile
    ln -s scripts/inspect-scopes.js vscode-textmate/inspect-scopes.js
    node vscode-textmate/inspect-scopes.js

update-submodules:
    git submodule update --init --recursive --remote
