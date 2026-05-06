# Tiny

A minimal terminal AI agent written in Rust.

## Install

```sh
cargo install --path .
```

## Config

Create `tiny.json` in your project directory or `~/.tiny/config.json`:

```json
{
  "api_key": "sk-...",
  "model": "gpt-4o-mini",
  "system": "You are a helpful assistant."
}
```

## Usage

```sh
tiny          # start or resume a session
```

## License

Apache-2.0
