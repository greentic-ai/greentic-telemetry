#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   LOCAL_CHECK_ONLINE=1 LOCAL_CHECK_STRICT=1 LOCAL_CHECK_VERBOSE=1 ci/local_check.sh
# Defaults: offline, non-strict, quiet.

LOCAL_CHECK_ONLINE="${LOCAL_CHECK_ONLINE:-0}"
LOCAL_CHECK_STRICT="${LOCAL_CHECK_STRICT:-0}"
LOCAL_CHECK_VERBOSE="${LOCAL_CHECK_VERBOSE:-0}"

if [[ "$LOCAL_CHECK_VERBOSE" == "1" ]]; then
  set -x
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SKIPPED=()
CLEANUP_CMDS=()
CRATE_VERSION=""

step() {
  echo ""
  echo "▶ $*"
}

have_tool() {
  command -v "$1" >/dev/null 2>&1
}

need() {
  local tool="$1"
  if have_tool "$tool"; then
    return 0
  fi
  echo "[miss] $tool" >&2
  return 1
}

register_cleanup() {
  CLEANUP_CMDS+=("$1")
}

cleanup() {
  local exit_code=$?
  for ((idx=${#CLEANUP_CMDS[@]}-1; idx>=0; idx--)); do
    eval "${CLEANUP_CMDS[idx]}"
  done
  exit "$exit_code"
}
trap cleanup EXIT

combine_reasons() {
  local combined=""
  for part in "$@"; do
    [[ -n "$part" ]] || continue
    if [[ -n "$combined" ]]; then
      combined="$combined; $part"
    else
      combined="$part"
    fi
  done
  echo "$combined"
}

missing_tools_reason() {
  local missing=()
  for tool in "$@"; do
    if ! have_tool "$tool"; then
      missing+=("$tool")
      if ! need "$tool"; then
        :
      fi
    fi
  done
  if ((${#missing[@]} > 0)); then
    printf "missing tool(s): %s" "${missing[*]}"
  fi
}

needs_online_reason() {
  if [[ "$LOCAL_CHECK_ONLINE" == "1" ]]; then
    echo ""
  else
    echo "requires network (set LOCAL_CHECK_ONLINE=1)"
  fi
}

require_env_reason() {
  local missing=()
  for var in "$@"; do
    if [[ -z "${!var:-}" ]]; then
      missing+=("$var")
    fi
  done
  if ((${#missing[@]} > 0)); then
    printf "missing env: %s" "${missing[*]}"
  fi
}

require_path_reason() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    printf "%s not found" "$path"
  fi
}

tool_exec_reason() {
  local tool="$1"
  shift || true
  local args=("$@")
  if ! have_tool "$tool"; then
    echo ""
    return
  fi
  if "$tool" "${args[@]}" >/dev/null 2>&1; then
    echo ""
  else
    printf "%s unusable (%s %s failed)" "$tool" "$tool" "${args[*]}"
  fi
}

run_or_skip() {
  local desc="$1"
  shift
  local reason="$1"
  shift

  if [[ -n "$reason" ]]; then
    if [[ "$LOCAL_CHECK_STRICT" == "1" ]]; then
      echo "[fail] $desc blocked: $reason"
      exit 1
    fi
    echo "[skip] $desc ($reason)"
    SKIPPED+=("$desc")
    return 0
  fi

  step "$desc"
  "$@"
}

extract_crate_version() {
  CRATE_VERSION=$(
    cargo metadata --no-deps --format-version 1 \
      | python3 -c "import json,sys; print(json.load(sys.stdin)['packages'][0]['version'])"
  )
  echo "crate_version=$CRATE_VERSION"
}

require_version_reason() {
  if [[ -z "$CRATE_VERSION" ]]; then
    echo "crate version unknown (metadata step skipped)"
  fi
}

simulate_release() {
  echo "Would create release greentic-telemetry v${CRATE_VERSION}"
}

validate_wit() {
  wasm-tools component wit "$REPO_ROOT/wit" >/dev/null
}

aws_otel_smoke() {
  local container="ci-local-aws-otel"
  local region="${AWS_REGION:-eu-west-1}"
  local config="$REPO_ROOT/.github/otel/aws.yaml"

  docker rm -f "$container" >/dev/null 2>&1 || true
  docker run -d --name "$container" -p 4317:4317 -p 4318:4318 \
    -e AWS_REGION="$region" \
    -v "$config":/etc/otelcol/config.yaml \
    public.ecr.aws/aws-observability/aws-otel-collector:latest \
    --config /etc/otelcol/config.yaml >/dev/null
  register_cleanup "docker rm -f $container >/dev/null 2>&1 || true"
  sleep 2

  local marker="greentic-aws-local-$RANDOM$RANDOM"
  OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4317" \
    OTEL_EXPORTER_OTLP_PROTOCOL="grpc" \
    SERVICE_NAME="greentic-telemetry" \
    TEST_MARKER="$marker" \
    cargo test --tests emit_cloud -- --nocapture

  local found=0
  for _ in $(seq 1 12); do
    if AWS_REGION="$region" AWS_DEFAULT_REGION="$region" \
      aws logs filter-log-events --log-group-name "/greentic/ci" \
      --filter-pattern "$marker" --max-items 1 \
      | jq -e '.events[0]' >/dev/null 2>&1; then
      found=1
      break
    fi
    sleep 10
  done
  if [[ "$found" -ne 1 ]]; then
    echo "Marker not found in CloudWatch"
    return 1
  fi

  local now
  now=$(date -u +%s)
  local start=$((now - 900))
  AWS_REGION="$region" AWS_DEFAULT_REGION="$region" \
    aws xray get-trace-summaries \
    --start-time "$start" \
    --end-time "$now" \
    | jq -e '.TraceSummaries | length >= 0' >/dev/null

  docker rm -f "$container" >/dev/null 2>&1 || true
}

azure_otel_smoke() {
  local container="ci-local-azure-otel"
  local config="$REPO_ROOT/.github/otel/azure.yaml"

  docker rm -f "$container" >/dev/null 2>&1 || true
  docker run -d --name "$container" -p 4317:4317 -p 4318:4318 \
    -e AZURE_APPINSIGHTS_INSTRUMENTATION_KEY="$AZURE_APPINSIGHTS_INSTRUMENTATION_KEY" \
    -v "$config":/etc/otelcol/config.yaml \
    otel/opentelemetry-collector:latest \
    --config /etc/otelcol/config.yaml >/dev/null
  register_cleanup "docker rm -f $container >/dev/null 2>&1 || true"
  sleep 2

  local marker="greentic-az-local-$RANDOM$RANDOM"
  OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4317" \
    OTEL_EXPORTER_OTLP_PROTOCOL="grpc" \
    SERVICE_NAME="greentic-telemetry" \
    TEST_MARKER="$marker" \
    cargo test --tests emit_cloud -- --nocapture

  if ! az extension show -n application-insights >/dev/null 2>&1; then
    az extension add -n application-insights >/dev/null
  fi

  local found=0
  for _ in $(seq 1 12); do
    if az monitor app-insights query \
      --app "$AZURE_APPINSIGHTS_APPID" \
      --analytics-query "traces | where tostring(message) has '${marker}' | take 1" \
      --offset 15m \
      | jq -e '.tables[0].rows[0]' >/dev/null 2>&1; then
      found=1
      break
    fi
    sleep 10
  done
  if [[ "$found" -ne 1 ]]; then
    echo "Marker not found in Azure traces"
    return 1
  fi

  docker rm -f "$container" >/dev/null 2>&1 || true
}

gcp_otel_smoke() {
  local container="ci-local-gcp-otel"
  local config="$REPO_ROOT/.github/otel/gcp.yaml"

  docker rm -f "$container" >/dev/null 2>&1 || true
  docker run -d --name "$container" -p 4317:4317 -p 4318:4318 \
    -e GCP_PROJECT_ID="${GCP_PROJECT_ID}" \
    -v "$config":/etc/otelcol/config.yaml \
    otel/opentelemetry-collector:latest \
    --config /etc/otelcol/config.yaml >/dev/null
  register_cleanup "docker rm -f $container >/dev/null 2>&1 || true"
  sleep 2

  local marker="greentic-gcp-local-$RANDOM$RANDOM"
  OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4317" \
    OTEL_EXPORTER_OTLP_PROTOCOL="grpc" \
    SERVICE_NAME="greentic-telemetry" \
    TEST_MARKER="$marker" \
    cargo test --tests emit_cloud -- --nocapture

  local found=0
  for _ in $(seq 1 12); do
    if gcloud logging read "textPayload:${marker}" \
      --limit=1 --freshness=15m --format=json \
      | jq -e '.[0]' >/dev/null 2>&1; then
      found=1
      break
    fi
    sleep 10
  done
  if [[ "$found" -ne 1 ]]; then
    echo "Marker not found in Cloud Logging"
    return 1
  fi

  gcloud trace list --limit=10 --format=json | jq -e 'length >= 0' >/dev/null
  docker rm -f "$container" >/dev/null 2>&1 || true
}

for core_tool in cargo rustc; do
  if ! need "$core_tool"; then
    echo "[err] $core_tool is required for local CI checks."
    exit 1
  fi
done

step "Toolchain details"
cargo --version
rustc --version
if have_tool rustfmt; then
  rustfmt --version || true
else
  echo "[warn] rustfmt missing; cargo fmt may fail"
fi
if have_tool wasm-tools; then
  wasm-tools --version || true
fi

fmt_reason="$(missing_tools_reason cargo)"
run_or_skip "cargo fmt --all -- --check" "$fmt_reason" \
  cargo fmt --all -- --check

clippy_reason="$(missing_tools_reason cargo)"
run_or_skip "cargo clippy --all-targets --all-features" "$clippy_reason" \
  cargo clippy --all-targets --all-features -- -D warnings

build_flags=("" "--no-default-features" "--all-features")
build_labels=("default features" "no default features" "all features")
for idx in "${!build_flags[@]}"; do
  flag="${build_flags[$idx]}"
  label="${build_labels[$idx]}"
  args=(cargo build --workspace --locked)
  if [[ -n "$flag" ]]; then
    args+=("$flag")
  fi
  build_reason="$(missing_tools_reason cargo)"
  run_or_skip "cargo build (${label})" "$build_reason" "${args[@]}"
done

test_reason="$(missing_tools_reason cargo)"
run_or_skip "cargo test --workspace --all-features --locked" "$test_reason" \
  cargo test --workspace --all-features --locked

if [[ -d "$REPO_ROOT/wit" ]]; then
  strict_wit_reason=""
  if [[ "$LOCAL_CHECK_STRICT" != "1" ]]; then
    strict_wit_reason="enable LOCAL_CHECK_STRICT=1 to validate WIT packages"
  fi
  wit_reason="$(combine_reasons "$(missing_tools_reason wasm-tools)" "$strict_wit_reason")"
  run_or_skip "wasm-tools wit validate" "$wit_reason" \
    validate_wit
fi

metadata_reason="$(missing_tools_reason cargo python3)"
run_or_skip "Extract crate version (cargo metadata)" "$metadata_reason" \
  extract_crate_version

publish_reason="$(combine_reasons "$(missing_tools_reason cargo)" "$(needs_online_reason)")"
run_or_skip "cargo publish --locked --dry-run --allow-dirty" "$publish_reason" \
  cargo publish --locked --dry-run --allow-dirty

release_reason="$(combine_reasons \
  "$(needs_online_reason)" \
  "$(require_env_reason GITHUB_TOKEN)" \
  "$(require_version_reason)")"
run_or_skip "Create GitHub release (dry-run)" "$release_reason" \
  simulate_release

aws_reason="$(combine_reasons \
  "$(missing_tools_reason cargo docker aws jq)" \
  "$(tool_exec_reason aws --version)" \
  "$(require_path_reason "$REPO_ROOT/.github/otel/aws.yaml")" \
  "$(needs_online_reason)")"
run_or_skip "AWS telemetry smoke" "$aws_reason" aws_otel_smoke

azure_reason="$(combine_reasons \
  "$(missing_tools_reason cargo docker az jq)" \
  "$(tool_exec_reason az --version)" \
  "$(require_path_reason "$REPO_ROOT/.github/otel/azure.yaml")" \
  "$(require_env_reason AZURE_APPINSIGHTS_INSTRUMENTATION_KEY AZURE_APPINSIGHTS_APPID)" \
  "$(needs_online_reason)")"
run_or_skip "Azure telemetry smoke" "$azure_reason" azure_otel_smoke

gcp_reason="$(combine_reasons \
  "$(missing_tools_reason cargo docker gcloud jq)" \
  "$(tool_exec_reason gcloud version)" \
  "$(require_path_reason "$REPO_ROOT/.github/otel/gcp.yaml")" \
  "$(require_env_reason GCP_PROJECT_ID)" \
  "$(needs_online_reason)")"
run_or_skip "GCP telemetry smoke" "$gcp_reason" gcp_otel_smoke

echo ""
if ((${#SKIPPED[@]} > 0)); then
  echo "[info] skipped ${#SKIPPED[@]} step(s):"
  for step_name in "${SKIPPED[@]}"; do
    echo "  - $step_name"
  done
fi
echo "[ok] local CI checks complete."
