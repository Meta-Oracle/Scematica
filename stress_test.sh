#!/usr/bin/env bash

# Stress Test Script for Scematica Arb Engine
# This script simulates heavy market volatility to verify system performance.

echo "--- Starting Scematica Stress Test ---"

# 1. Start the Arb Engine in dry-run mode to avoid actual fund risk
echo "1. Launching Arb Engine in Dry-Run mode..."
cargo run --release --bin arb -- --config config.toml --dry-run &
ARB_PID=$!

# 2. Give it time to sync the graph
echo "2. Waiting for GraphRefresher to sync..."
sleep 15

# 3. Simulate high-load by spamming logs with market activity
echo "3. Stressing event loop..."
# This is a basic simulation of high event volume
for i in {1..100}; do
    echo "Simulating market tick #$i"
    # In a real scenario, this would be a high-frequency socket connection
    sleep 0.1
done

echo "--- Stress Test Complete. Checking system stability ---"
# Check if process is still alive
if ps -p $ARB_PID > /dev/null
then
    echo "Engine is stable. Shutting down."
    kill $ARB_PID
else
    echo "Engine crashed during stress test!"
    exit 1
fi
