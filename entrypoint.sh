#!/bin/bash
# Entrypoint: runs facilitator + all demo bots in one container.
# Facilitator runs in foreground (serves HTTP).
# Bots run in background (issue invoices on loops).

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

cd /opt/signal-bot
export FACILITATOR_URL=http://localhost:8080

# Start signal-bot (if key is set)
if [ -n "$SIGNAL_BOT_PK" ]; then
    echo "[entrypoint] Starting signal-bot..."
    node dist/signal-bot.js &
    echo "[entrypoint] Signal bot started"
fi

# Start compute-bot (if key is set)
if [ -n "$COMPUTE_BOT_PK" ]; then
    echo "[entrypoint] Starting compute-bot..."
    node dist/compute-bot.js &
    echo "[entrypoint] Compute bot started"
fi

# Start analytics-bot (if key is set)
if [ -n "$ANALYTICS_BOT_PK" ]; then
    echo "[entrypoint] Starting analytics-bot..."
    node dist/analytics-bot.js &
    echo "[entrypoint] Analytics bot started"
fi

# Wait for facilitator (if it dies, container exits)
wait $FACILITATOR_PID
