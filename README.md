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
  "provider": "openai",
  "api_key": "sk-...",
  "model": "gpt-4o-mini"
}
```

If no provider is configured, tiny uses OpenAI when `api_key` or
`OPENAI_API_KEY` is set. Otherwise it starts with the local `llama_cpp`
provider.

For llama.cpp, start `llama-server` and point tiny at its OpenAI-compatible
chat completions endpoint:

```json
{
  "provider": "llama_cpp",
  "base_url": "http://127.0.0.1:8080",
  "model": "local"
}
```

Tool calls require a llama.cpp chat template/model that supports OpenAI-style
function calling, for example by starting `llama-server` with `--jinja`.

## Usage

```sh
tiny          # start or resume a session
```

## License

Apache-2.0
