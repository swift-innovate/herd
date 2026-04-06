# Herd-Tune: Node Auto-Registration for Herd

## Overview

Herd needs a frictionless way for operators to add Ollama backend nodes to their fleet. The workflow:

1. Operator opens the Herd dashboard in a browser (from any machine, including the node itself)
2. Clicks "Add Node" — dashboard offers a download of the appropriate `herd-tune` script (PowerShell for Windows, bash for Linux), pre-configured with this Herd instance's registration endpoint
3. Operator runs the script on the Ollama node
4. Script detects local GPU/VRAM/RAM, probes the local Ollama instance, applies recommended `OLLAMA_*` environment variables, and POSTs a registration payload to Herd
5. Node appears in the dashboard fleet view immediately

No SSH. No file uploads. No manual config editing. The script is the only thing that touches the node. Herd only ever talks to nodes via their Ollama HTTP API.

## Architecture

```
[Herd Dashboard]
  │
  ├── GET /dashboard/add-node          → "Add Node" page with download buttons
  ├── GET /api/nodes/script?os=windows → Returns herd-tune.ps1 with endpoint baked in
  ├── GET /api/nodes/script?os=linux   → Returns herd-tune.sh with endpoint baked in
  │
  ├── POST /api/nodes/register         → Receives node registration from herd-tune
  ├── GET /api/nodes                   → List all registered nodes
  ├── GET /api/nodes/:id               → Single node detail
  ├── PUT /api/nodes/:id               → Update node (priority, tags, enabled)
  ├── DELETE /api/nodes/:id            → Remove node from fleet
  │
  └── Background: health poller polls each node's Ollama /api/ps every N seconds
```

## API Endpoints

### POST /api/nodes/register

Called by `herd-tune` after local detection. Registers or updates a node.

**Request:**
```json
{
  "hostname": "citadel",
  "ollama_url": "http://192.168.1.100:11434",
  "gpu": "NVIDIA GeForce RTX 5090",
  "vram_mb": 32768,
  "ram_mb": 131072,
  "ollama_version": "0.16.1",
  "models_available": 42,
  "models_loaded": ["qwen3:32b", "gemma3:27b"],
  "recommended_config": {
    "num_parallel": 8,
    "max_loaded_models": 4,
    "max_queue": 1024,
    "keep_alive": "30m",
    "flash_attention": true,
    "kv_cache_type": "q8_0",
    "context_length": 16384
  },
  "config_applied": true,
  "herd_tune_version": "0.3.0",
  "os": "windows",
  "registered_at": "2026-03-29T14:30:00Z"
}
```

**Response (201 Created or 200 OK if re-registering):**
```json
{
  "id": "node-uuid-here",
  "hostname": "citadel",
  "status": "registered",
  "message": "Node registered successfully. Health polling started."
}
```

**Behavior:**
- If a node with the same `hostname` already exists, update it (re-registration is idempotent)
- Start health polling immediately on successful registration
- Store in SQLite (consistent with Herd's existing data layer)

### GET /api/nodes/script?os={windows|linux}

Returns the `herd-tune` script with the Herd registration endpoint pre-configured.

**Behavior:**
- Read the script template from an embedded resource or file
- Replace the placeholder endpoint URL with the actual Herd base URL (derived from the request's Host header or a configured public URL)
- Set `Content-Disposition: attachment; filename="herd-tune.ps1"` (or `.sh`)
- Set appropriate `Content-Type`

**Example:** If Herd is running at `http://192.168.1.50:8081`, the downloaded script will have `$HerdEndpoint = "http://192.168.1.50:8081"` baked in.

### GET /api/nodes

Returns all registered nodes with current health status.

```json
{
  "nodes": [
    {
      "id": "uuid",
      "hostname": "citadel",
      "ollama_url": "http://192.168.1.100:11434",
      "gpu": "NVIDIA GeForce RTX 5090",
      "vram_mb": 32768,
      "ram_mb": 131072,
      "max_concurrent": 8,
      "status": "healthy",
      "models_loaded": ["qwen3:32b"],
      "models_available": 42,
      "last_health_check": "2026-03-29T14:35:00Z",
      "registered_at": "2026-03-29T14:30:00Z",
      "priority": 1,
      "enabled": true,
      "tags": ["local"]
    }
  ]
}
```

### PUT /api/nodes/:id

Update operator-controlled fields: `priority`, `tags`, `enabled`.
Hardware fields are only updated via re-registration (re-run `herd-tune`).

### DELETE /api/nodes/:id

Remove a node from the fleet. Stops health polling. In-flight requests to this node should drain gracefully.

## Health Polling

After registration, Herd polls each node on a configurable interval (default 10s):

1. `GET {ollama_url}/api/ps` → loaded models, VRAM usage, context length, expiration
2. `GET {ollama_url}/api/tags` → available models (less frequent, every 60s)
3. Track response latency as a routing signal

**Node status states:**
- `healthy` — responding, models loaded
- `degraded` — responding but high latency or VRAM pressure
- `unreachable` — failed health checks (after 3 consecutive failures)
- `disabled` — operator manually disabled via dashboard

Unreachable nodes are not removed — they stay registered but excluded from routing. They automatically return to `healthy` when they start responding again.

## herd-tune Scripts

### PowerShell (Windows) — herd-tune.ps1

Core logic:

- **Detect** GPU (nvidia-smi, WMI fallback), RAM, local Ollama via API
- **Calculate** recommended `OLLAMA_*` settings based on VRAM
- **Apply** (with `-Apply` flag) — set Machine-level env vars + restart Ollama service
- **Register** — POST payload to `{Herd}/api/nodes/register`
- **Fallback** — if `-Herd` not set and no baked endpoint, skip registration, do local detection only

### Bash (Linux) — herd-tune.sh

Same logic, adapted:

- `nvidia-smi` for GPU, `free -m` for RAM
- `systemctl` for Ollama service management
- `curl` for registration POST

### Script Template Placeholders

Both scripts have a clearly marked placeholder at the top:

```powershell
# ── Herd Registration (auto-configured on download) ──
$HerdEndpoint = "%%HERD_ENDPOINT%%"
```

```bash
# ── Herd Registration (auto-configured on download) ──
HERD_ENDPOINT="%%HERD_ENDPOINT%%"
```

The `/api/nodes/script` endpoint replaces `%%HERD_ENDPOINT%%` with the real URL before serving.

## Dashboard UI — Add Node Flow

### "Add Node" page (`/dashboard/add-node` or section in settings)

**Content:**

1. Brief explanation: "Run the herd-tune script on any machine running Ollama to add it to your fleet."

2. Two download buttons:
   - **Windows (PowerShell)** → `GET /api/nodes/script?os=windows`
   - **Linux (Bash)** → `GET /api/nodes/script?os=linux`

3. Quick-start instructions shown inline:

   **Windows:**
   ```
   1. Download herd-tune.ps1 (button above)
   2. Open PowerShell as Administrator on your Ollama machine
   3. Run:  .\herd-tune.ps1 -Apply
   4. Done — your node will appear in the fleet below
   ```

   **Linux:**
   ```
   1. Download herd-tune.sh (button above)
   2. On your Ollama machine:
      chmod +x herd-tune.sh
      sudo ./herd-tune.sh --apply
   3. Done — your node will appear in the fleet below
   ```

4. **Live fleet table** below the instructions — shows registered nodes updating in real-time (poll `/api/nodes` or use SSE). Operator sees the node appear after running the script.

### Fleet Management (existing dashboard, or new section)

- Table of nodes: hostname, GPU, VRAM, status, loaded models, parallel slots, priority
- Toggle enabled/disabled per node
- Edit priority and tags
- Remove node
- "Re-tune" button that shows the command to re-run `herd-tune` on that node

## Data Storage

Add a `nodes` table to Herd's SQLite database:

```sql
CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    hostname TEXT NOT NULL UNIQUE,
    ollama_url TEXT NOT NULL,
    gpu TEXT,
    vram_mb INTEGER DEFAULT 0,
    ram_mb INTEGER DEFAULT 0,
    max_concurrent INTEGER DEFAULT 1,
    ollama_version TEXT,
    os TEXT,
    status TEXT DEFAULT 'healthy',
    priority INTEGER DEFAULT 10,
    enabled INTEGER DEFAULT 1,
    tags TEXT DEFAULT '[]',
    models_available INTEGER DEFAULT 0,
    models_loaded TEXT DEFAULT '[]',
    recommended_config TEXT DEFAULT '{}',
    config_applied INTEGER DEFAULT 0,
    last_health_check TEXT,
    registered_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

## Integration with Routing

The node registry replaces the current static backend configuration. Herd's router should:

1. Query the `nodes` table for `enabled = 1 AND status IN ('healthy', 'degraded')`
2. Use `max_concurrent` to know how many parallel slots each node has
3. Use health poll data (loaded models, response latency) for model affinity and least-loaded routing
4. Respect `priority` for tie-breaking

This means the `[[backends]]` section in the TOML/YAML config becomes optional — nodes registered via `herd-tune` are the primary source. Static config backends still work for non-Ollama backends (vLLM, cloud providers) that don't run `herd-tune`.

## Implementation Order

1. **SQLite `nodes` table** — migration in Herd
2. **POST /api/nodes/register** — accept and store registrations
3. **GET /api/nodes** — list nodes (dashboard needs this)
4. **Health poller** — background task polling registered nodes
5. **Router integration** — read from `nodes` table instead of (or in addition to) static config
6. **Script templates** — embed herd-tune.ps1 and herd-tune.sh as resources
7. **GET /api/nodes/script** — serve scripts with endpoint baked in
8. **Dashboard UI** — add node page, fleet table, management controls
9. **PUT/DELETE /api/nodes/:id** — management endpoints
10. **herd-tune scripts** — finalize with registration POST

## Notes

- Scripts ship embedded in the Herd binary/container, not as separate downloads. When the container is built, the scripts are bundled.
- Registration is idempotent. Running `herd-tune` again on a node updates its entry. This is how you "re-tune" after hardware changes.
- The dashboard "Add Node" instructions should be visible even with zero nodes registered (first-run experience).
- Herd's public URL / external address may differ from its container internal address. Consider a `HERD_PUBLIC_URL` env var for generating correct script endpoints.
- The scripts should detect the Ollama URL on the local machine and prefer Tailscale IP > LAN IP > localhost for the `ollama_url` field, since Herd needs to reach it from the container network.
