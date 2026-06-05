@echo off
REM Scematica v1.11.0 - ScemaDEX Peer-Mesh Relay Launcher
REM Serves the inference/experience mesh + signal oracle over HTTP, so agents
REM can trade bonded inferences and learned experience (optionally x402-gated).

echo ========================================
echo ScemaDEX Relay (peer mesh + signal oracle)
echo ========================================
echo.
echo Serving the RemotePeerMarket contract + signal endpoints.
echo Point a net-feature RemotePeerMarket client at this host to join the mesh.
echo Press Ctrl+C to stop
echo.

cargo run --release --bin scemadex-relay -- %*

pause
