# https://just.systems

generate-dump:
    cargo run --release --bin=build-registry --features=tools

update-submodules:
    git submodule update --init --recursive --remote
