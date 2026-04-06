#!/usr/bin/env bash
set -euo pipefail

# ── Herd Pro Registration (auto-configured on download) ──
HERD_PRO_ENDPOINT="%%HERD_PRO_ENDPOINT%%"
ENROLLMENT_KEY="%%ENROLLMENT_KEY%%"
HERD_TUNE_VERSION="0.8.0"
APPLY=false

# Parse args
while [[ $# -gt 0 ]]; do
    case "$1" in
        --apply) APPLY=true; shift ;;
        --herd-pro) HERD_PRO_ENDPOINT="$2"; shift 2 ;;
        --herd-pro=*) HERD_PRO_ENDPOINT="${1#*=}"; shift ;;
        --enrollment-key) ENROLLMENT_KEY="$2"; shift 2 ;;
        --enrollment-key=*) ENROLLMENT_KEY="${1#*=}"; shift ;;
        -h|--help)
            echo "Usage: $0 [--apply] [--herd-pro URL] [--enrollment-key KEY]"
            echo "  --apply           Apply recommended OLLAMA_* env vars and restart Ollama"
            echo "  --herd-pro        Herd Pro endpoint URL for registration"
            echo "  --enrollment-key  Enrollment key for node registration"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo ""
echo "  _               _       _"
echo " | |_  ___ _ _ __| |  ___| |_ _  _ _ _  ___"
echo " | ' \/ -_) '_/ _\` | |___| _| || | ' \/ -_)"
echo " |_||_\___|_| \__,_|     \__|\_,_|_||_\___|"
echo ""
echo "  GPU Detection & Ollama Configuration"
echo "  Version $HERD_TUNE_VERSION"
echo ""

# ── Detect GPU ──
echo "=== Hardware Detection ==="
GPU_NAME=""
VRAM_MB=0

if command -v nvidia-smi &>/dev/null; then
    GPU_INFO=$(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>/dev/null || true)
    if [ -n "$GPU_INFO" ]; then
        GPU_NAME=$(echo "$GPU_INFO" | head -1 | cut -d',' -f1 | xargs)
        VRAM_MB=$(echo "$GPU_INFO" | head -1 | cut -d',' -f2 | xargs)
    fi
fi

# Detect RAM
RAM_MB=$(free -m 2>/dev/null | awk '/^Mem:/{print $2}' || echo 0)

echo "  GPU:  ${GPU_NAME:-Not detected}"
echo "  VRAM: ${VRAM_MB} MB"
echo "  RAM:  ${RAM_MB} MB"

# ── Detect Ollama ──
echo ""
echo "=== Ollama Detection ==="
OLLAMA_URL="http://localhost:11434"

OLLAMA_VERSION=$(curl -sf "${OLLAMA_URL}/api/version" 2>/dev/null | grep -o '"version":"[^"]*"' | cut -d'"' -f4 || true)
if [ -z "$OLLAMA_VERSION" ]; then
    echo "ERROR: Ollama is not running at ${OLLAMA_URL}"
    exit 1
fi

# Get loaded models
MODELS_LOADED=$(curl -sf "${OLLAMA_URL}/api/ps" 2>/dev/null | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    names = [m['name'] for m in data.get('models', [])]
    print(json.dumps(names))
except: print('[]')
" 2>/dev/null || echo '[]')

# Get available model count
MODELS_AVAILABLE=$(curl -sf "${OLLAMA_URL}/api/tags" 2>/dev/null | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(len(data.get('models', [])))
except: print(0)
" 2>/dev/null || echo 0)

# Determine best reachable URL (prefer LAN IP over localhost)
BEST_URL="$OLLAMA_URL"
LAN_IP=$(hostname -I 2>/dev/null | awk '{print $1}' || true)
if [ -n "$LAN_IP" ]; then
    BEST_URL="http://${LAN_IP}:11434"
fi

echo "  URL:      ${BEST_URL}"
echo "  Version:  ${OLLAMA_VERSION}"
echo "  Models:   ${MODELS_AVAILABLE} available"

# ── Calculate recommended config ──
echo ""
echo "=== Recommended Configuration ==="

if [ "$VRAM_MB" -ge 24576 ]; then
    NUM_PARALLEL=8; MAX_LOADED=4; MAX_QUEUE=1024; KEEP_ALIVE="30m"; CTX_LEN=16384
elif [ "$VRAM_MB" -ge 12288 ]; then
    NUM_PARALLEL=4; MAX_LOADED=2; MAX_QUEUE=512; KEEP_ALIVE="15m"; CTX_LEN=8192
elif [ "$VRAM_MB" -ge 8192 ]; then
    NUM_PARALLEL=2; MAX_LOADED=1; MAX_QUEUE=256; KEEP_ALIVE="10m"; CTX_LEN=4096
else
    NUM_PARALLEL=1; MAX_LOADED=1; MAX_QUEUE=128; KEEP_ALIVE="5m"; CTX_LEN=2048
fi

echo "  num_parallel:     $NUM_PARALLEL"
echo "  max_loaded_models: $MAX_LOADED"
echo "  max_queue:        $MAX_QUEUE"
echo "  keep_alive:       $KEEP_ALIVE"
echo "  flash_attention:  true"
echo "  kv_cache_type:    q8_0"
echo "  context_length:   $CTX_LEN"

# ── Apply config ──
CONFIG_APPLIED=false
if [ "$APPLY" = true ]; then
    echo ""
    echo "=== Applying Ollama Configuration ==="

    # Write to /etc/environment or systemd override
    OLLAMA_ENV_FILE="/etc/systemd/system/ollama.service.d/override.conf"
    mkdir -p "$(dirname "$OLLAMA_ENV_FILE")"

    cat > "$OLLAMA_ENV_FILE" << ENVEOF
[Service]
Environment="OLLAMA_NUM_PARALLEL=${NUM_PARALLEL}"
Environment="OLLAMA_MAX_LOADED_MODELS=${MAX_LOADED}"
Environment="OLLAMA_MAX_QUEUE=${MAX_QUEUE}"
Environment="OLLAMA_KEEP_ALIVE=${KEEP_ALIVE}"
Environment="OLLAMA_FLASH_ATTENTION=1"
Environment="OLLAMA_KV_CACHE_TYPE=q8_0"
Environment="OLLAMA_CONTEXT_LENGTH=${CTX_LEN}"
ENVEOF

    echo "  Wrote ${OLLAMA_ENV_FILE}"

    # Restart Ollama
    echo "  Restarting Ollama service..."
    systemctl daemon-reload
    systemctl restart ollama
    sleep 3
    echo "  Ollama service restarted."
    CONFIG_APPLIED=true
else
    echo ""
    echo "Run with --apply to set these environment variables and restart Ollama."
fi

# ── Generate stable machine ID ──
NODE_ID=""
if [ -f /etc/machine-id ]; then
    NODE_ID=$(cat /etc/machine-id)
elif [ -f /var/lib/dbus/machine-id ]; then
    NODE_ID=$(cat /var/lib/dbus/machine-id)
else
    MAC=$(ip link show 2>/dev/null | awk '/ether/{print $2; exit}' || true)
    NODE_ID=$(echo -n "${MAC}$(hostname)" | sha256sum | cut -d' ' -f1 | head -c 32)
fi

# ── Register with Herd Pro ──
if [ -n "$HERD_PRO_ENDPOINT" ] && [ "$HERD_PRO_ENDPOINT" != '%%HERD_PRO_ENDPOINT%%' ]; then
    echo ""
    echo "=== Registering with Herd Pro ==="
    echo "  Endpoint: ${HERD_PRO_ENDPOINT}"

    REG_URL="${HERD_PRO_ENDPOINT}/api/nodes/register"
    if [ -n "$ENROLLMENT_KEY" ] && [ "$ENROLLMENT_KEY" != '%%ENROLLMENT_KEY%%' ]; then
        REG_URL="${REG_URL}?enrollment_key=${ENROLLMENT_KEY}"
    fi

    HOSTNAME_VAL=$(hostname | tr '[:upper:]' '[:lower:]')
    REGISTERED_AT=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    NODE_ID_JSON=""
    if [ -n "$NODE_ID" ]; then
        NODE_ID_JSON="\"node_id\": \"${NODE_ID}\","
    fi

    PAYLOAD=$(cat << JSONEOF
{
  ${NODE_ID_JSON}
  "hostname": "${HOSTNAME_VAL}",
  "ollama_url": "${BEST_URL}",
  "gpu": "${GPU_NAME}",
  "vram_mb": ${VRAM_MB},
  "ram_mb": ${RAM_MB},
  "ollama_version": "${OLLAMA_VERSION}",
  "models_available": ${MODELS_AVAILABLE},
  "models_loaded": ${MODELS_LOADED},
  "recommended_config": {
    "num_parallel": ${NUM_PARALLEL},
    "max_loaded_models": ${MAX_LOADED},
    "max_queue": ${MAX_QUEUE},
    "keep_alive": "${KEEP_ALIVE}",
    "flash_attention": true,
    "kv_cache_type": "q8_0",
    "context_length": ${CTX_LEN}
  },
  "config_applied": ${CONFIG_APPLIED},
  "herd_tune_version": "${HERD_TUNE_VERSION}",
  "os": "linux",
  "registered_at": "${REGISTERED_AT}"
}
JSONEOF
)

    RESPONSE=$(curl -sf -X POST "${REG_URL}" \
        -H "Content-Type: application/json" \
        -d "$PAYLOAD" 2>/dev/null || true)

    if [ -n "$RESPONSE" ]; then
        echo "  Registration successful!"
        echo "  $RESPONSE"
    else
        echo "  WARNING: Registration failed. You can register later with --herd-pro <url>"
    fi
else
    echo ""
    echo "No Herd Pro endpoint configured. Run with --herd-pro <url> to register."
fi

echo ""
echo "Done!"
