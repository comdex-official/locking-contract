# Locking Contract

The locking-contract encloses the core functionality for locking governance tokens
for a different locking period generating ve representation of locked tokens in an nft.
The contract also holds the key logic to handle token emission and its distribution.

## Documentation

For an overview of the locking contract, click [here](/docs/OVERVIEW.md).
Documentation of the [instantiate](/docs/INSTANTIATE.md), [execute](/docs/EXECUTE.md) and
[query](/docs/QUERY.md) messages are available in `docs/` folder.

## Compilation

For compiling, you may use the following commands, with the latter
generating a much more optimized wasm file (though it does require docker).

```sh
RUSTFLAGS='-C link-arg=-s' cargo wasm
```

```sh
docker run --rm -v "$(pwd)":/code \
  --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target \
  --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
  cosmwasm/rust-optimizer:0.12.6
```
