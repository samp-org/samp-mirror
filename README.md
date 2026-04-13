<h1 align="center">SAMP Mirror</h1>

<p align="center">
  <strong>Index and serve SAMP remarks from any Substrate chain.</strong>
</p>

<p align="center">
  <a href="https://codecov.io/gh/samp-org/samp-mirror"><img src="https://codecov.io/gh/samp-org/samp-mirror/graph/badge.svg" alt="codecov" /></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> •
  <a href="#cli">CLI</a> •
  <a href="#api">API</a> •
  <a href="#docker">Docker</a>
</p>

---

Connects to a Substrate node, indexes all [SAMP](https://github.com/samp-org/samp) remarks into SQLite, and serves them via HTTP API. Clients use mirrors to discover messages without scanning the full chain. Mirrors never see decrypted content. Clients verify all data against the chain.

## Quick start

```
cargo build --release
./target/release/samp-mirror --node <node-ws-url>
```

The mirror detects the chain name and SS58 prefix, catches up on historical blocks, then subscribes to new finalized blocks.

## CLI

```
samp-mirror --node <URL> [--db <PATH>] [--port <PORT>] [--start-block <N>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--node` | (required) | Substrate WebSocket RPC endpoint |
| `--db` | `mirror.db` | SQLite database path |
| `--port` | `8080` | HTTP API port |
| `--start-block` | `0` | Block number to start indexing from |

### Snapshot

Export the database as a compressed archive:

```
samp-mirror snapshot --db mirror.db --output snapshot.tar.gz
```

## API

All list endpoints accept `?after=N` to return only remarks from blocks after N.

### `GET /v1/health`

```json
{
  "chain": "Bittensor",
  "ss58_prefix": 42,
  "synced_to": 2081,
  "version": "2.0.0"
}
```

### `GET /v1/channels`

All channel creation remarks (`0x13`).

```json
[
  {
    "block": 100,
    "index": 2,
    "creator": "5Grw...",
    "name": "general",
    "description": "General discussion",
    "timestamp": 1775344320
  }
]
```

### `GET /v1/channels/:block/:index/messages?after=0`

Messages for a specific channel (`0x14`).

```json
[
  {
    "block": 150,
    "index": 0,
    "sender": "5FHn...",
    "timestamp": 1775345000,
    "remark": "14640000..."
  }
]
```

### `GET /v1/remarks?type=0x11&after=0`

Remarks by content type.

```json
[
  {
    "block": 200,
    "index": 1,
    "sender": "5Grw...",
    "timestamp": 1775346000,
    "remark": "1132a1b2..."
  }
]
```

Content types: `0x10` (public), `0x11` (encrypted), `0x12` (thread), `0x13` (channel create), `0x14` (channel), `0x15` (group).

### `GET /v1/remarks?sender=5Grw...&after=0`

Remarks by sender address. Same response format.

## Docker

```
docker compose up
```

Edit `docker-compose.yml` to set your node URL. Data persists in `./data/`.

## PM2

```
cargo build --release
pm2 start ecosystem.config.js
```

Edit `ecosystem.config.js` to set your node URL.

## License

MIT. See [LICENSE](LICENSE).
