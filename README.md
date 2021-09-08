# Looper

> A CLI tool that plays songs on loop so you can get in the zone

## Building a Binary for macOS (assuming x86_64)

```shell
cargo build --target=x86_64-apple-darwin --release
```

## Commands


```shell
looper play --url "/your/long/path/here/play_that_funky_music.mp3"
```

```shell
cargo run -- play --url tests/fixtures/sound.mp3 # try it out quickly without building for your OS
```
