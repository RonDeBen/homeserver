#!/usr/bin/env bash
# Run the gateway locally with cargo-watch while Postgres and NATS run in Docker.
#
# Usage:
#   scripts/gateway-dev.sh start    # start infra and the watcher in the background
#   scripts/gateway-dev.sh stop     # stop the watcher and infra containers
#   scripts/gateway-dev.sh restart
#   scripts/gateway-dev.sh status
#   scripts/gateway-dev.sh logs     # follow the watcher log

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PID_FILE="$ROOT/.gateway-watch.pid"
LOG_FILE="$ROOT/.gateway-watch.log"

cd "$ROOT"

# Load the host-side values from .env when present. These defaults match
# .env.example and the ports published by docker-compose.yml.
if [[ -f "$ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT/.env"
  set +a
fi
: "${DATABASE_URL:=postgres://homeserver:homeserver@localhost:5433/homeserver}"
: "${NATS_URL:=nats://localhost:4222}"
: "${AUTH_MODE:=trusted}"

watcher_running() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid="$(<"$PID_FILE")"
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  ps -p "$pid" -o command= 2>/dev/null | grep -q '[c]argo watch'
}

clear_stale_pid() {
  if [[ -f "$PID_FILE" ]] && ! watcher_running; then
    rm -f "$PID_FILE"
  fi
}

start() {
  clear_stale_pid
  if watcher_running; then
    echo "gateway watcher is already running (pid $(<"$PID_FILE"))"
    return 0
  fi

  if ! command -v cargo-watch >/dev/null 2>&1; then
    echo "cargo-watch is required. Install it with: cargo install cargo-watch" >&2
    exit 1
  fi

  echo "Starting Postgres and NATS..."
  docker compose up -d nats postgres

  echo "Starting gateway watcher (log: $LOG_FILE)"
  DATABASE_URL="$DATABASE_URL" \
  NATS_URL="$NATS_URL" \
  AUTH_MODE="$AUTH_MODE" \
  cargo watch \
    -w crates/gateway \
    -w crates/common \
    -w Cargo.toml \
    -w Cargo.lock \
    -x 'run -p gateway' >"$LOG_FILE" 2>&1 &
  echo $! >"$PID_FILE"

  echo "Gateway watcher started (pid $(<"$PID_FILE")); dashboard: http://localhost:8080"
  echo "Use '$0 logs' to follow output or '$0 stop' to stop it."
}

stop() {
  clear_stale_pid
  if [[ -f "$PID_FILE" ]]; then
    local pid
    pid="$(<"$PID_FILE")"
    echo "Stopping gateway watcher (pid $pid)..."
    kill -TERM "$pid" 2>/dev/null || true
    rm -f "$PID_FILE"
  else
    echo "Gateway watcher is not running."
  fi

  echo "Stopping Postgres and NATS containers..."
  docker compose stop nats postgres
}

status() {
  clear_stale_pid
  if watcher_running; then
    echo "gateway watcher: running (pid $(<"$PID_FILE"))"
  else
    echo "gateway watcher: stopped"
  fi
  docker compose ps nats postgres
}

case "${1:-}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) status ;;
  logs) exec tail -f "$LOG_FILE" ;;
  *)
    echo "Usage: $0 {start|stop|restart|status|logs}" >&2
    exit 2
    ;;
esac
