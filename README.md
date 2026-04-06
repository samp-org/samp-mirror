# samp-mirror

Indexes [SAMP](https://github.com/samp-org/samp) remarks from a Substrate blockchain and serves them via HTTP API. Works with any Substrate chain that has `system.remark_with_event`.

## Quick start

```
cargo build --release
./target/release/samp-mirror --node <your-substrate-node-ws-url>
```

The mirror connects to the node, detects the chain name and SS58 prefix, catches up on historical blocks, then subscribes to new finalized blocks. Remarks are stored in a local SQLite database.

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

Exports the database as a compressed `.tar.gz` archive.

```
samp-mirror snapshot --db mirror.db --output snapshot.tar.gz
```

## API

All list endpoints accept `?after=N` to return only remarks from blocks after N.

### `GET /v1/health`

```json
{
  "chain": "MyChain",
  "ss58_prefix": 42,
  "synced_to": 2081,
  "version": "1.0.0"
}
```

### `GET /v1/channels`

All discovered channel creation remarks (`0x13`).

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

Remarks by sender SS58 address. Same response format as above.

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

```
MIT License

Copyright (c) 2025 Maciej Kula

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
