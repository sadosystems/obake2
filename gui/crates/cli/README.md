# Cli

## Testing

You can test your changes to the `cli` crate by first building the main obake binary:

```
cargo build -p obake
```

And then building and running the `cli` crate with the following parameters:

```
 cargo run -p cli -- --obake ./target/debug/obake.exe
```
