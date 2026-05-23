#!/bin/bash
# Entrypoint: runs facilitator + signal-bot in one container.
# Facilitator runs in foreground (serves HTTP).
# Signal bot runs in background (issues invoices on a loop).

set -e

echo "[entrypoint] Starting EMEI Facilitator..."
emei-server &
FACILITATOR_PID=$!

# Wait for facilitator to be ready
echo "[entrypoint] Waiting for facilitator to start..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
        echo "[entrypoint] Facilitator is ready!"
        break
    fi
    sleep 1
done

# Start signal bot in background (only if SIGNAL_BOT_PK is set)
if [ -n "$SIGNAL_BOT_PK" ]; then
    echo "[entrypoint] Starting signal-bot..."
    cd /opt/signal-bot
    # Point bot at localhost since they're in the same container
    export FACILITATOR_URL=http://localhost:8080
    node dist/signal-bot.js &
    SIGNAL_PID=$!
    echo "[entrypoint] Signal bot started (PID: $SIGNAL_PID)"
fi

# Wait for facilitator (if it dies, container exits)
wait $FACILITATOR_PID
