# Elastic Dev Bundle

Local Elastic stack for inspecting Greentic telemetry.

## Usage

1. Start the stack:
   ```bash
   docker compose -f dev/elastic-compose/docker-compose.yml up -d
   ```
2. Configure your app to export OTLP spans:
   ```bash
   export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
   ```
3. Open Kibana at <http://localhost:5601/> to explore indices.

The included OpenTelemetry Collector forwards OTLP gRPC/HTTP traffic to its logging exporter for quick inspection. Modify `otel-config.yaml` if you want to forward traces to Elastic APM.

> Dev use only. Do not run this bundle in production or ship credentials with it.
