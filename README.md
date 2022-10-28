# Locking Contract

TODO: Add a brief overview of locking contract.

## Documentation

TODO: Add hyperlinks to other docs.

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

TODO: Add basic compilation information.
