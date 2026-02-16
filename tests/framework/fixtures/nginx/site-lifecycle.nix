{
  nginxInit,
  nginxStart,
  nginxStop,
  nginxReload,
  nginxCheckConfig,
  nginxStatus,
  nginxHealth,
  nginxReady,
  nginxSiteAdd,
  nginxSiteStatic,
  nginxSiteList,
  nginxSiteDisable,
  nginxSiteEnable,
  nginxSiteRemove,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  wait_ok() {
    local attempts="$1"
    local interval="$2"
    shift 2
    local i
    for i in $(seq 1 "$attempts"); do
      if "$@" >/dev/null 2>&1; then
        return 0
      fi
      sleep "$interval"
    done
    return 1
  }

  wait_fail() {
    local attempts="$1"
    local interval="$2"
    shift 2
    local i
    for i in $(seq 1 "$attempts"); do
      if "$@" >/dev/null 2>&1; then
        sleep "$interval"
      else
        return 0
      fi
    done
    return 1
  }

  cleanup_nginx() {
    ${nginxStop} >/dev/null 2>&1 || true
    if [ -n "''${NGINX_PID:-}" ] && kill -0 "$NGINX_PID" 2>/dev/null; then
      stop_service "$NGINX_PID" "nginx"
    fi
  }

  eval "$("$SLOT_INFO")"

  set +e
  ${nginxReload} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "nginxReload should fail before init"

  ${nginxInit}
  ${nginxInit}
  NGINX_DIR="$BASE_DIR/nginx-$SLOT-$ENV"

  [ -f "$NGINX_DIR/conf/nginx.conf" ] || fail "missing nginx.conf"

  ${nginxCheckConfig} >/dev/null || fail "nginxCheckConfig failed"

  ${nginxSiteAdd} "example.localhost" "127.0.0.1" "$BACKEND_PORT"
  [ -f "$NGINX_DIR/conf/sites-available/example.localhost.conf" ] || fail "site-add did not create available config"
  [ -e "$NGINX_DIR/conf/sites-enabled/example.localhost.conf" ] || fail "site-add did not enable site"

  STATIC_ROOT="$NGINX_DIR/static-site"
  mkdir -p "$STATIC_ROOT"
  echo "ok" > "$STATIC_ROOT/index.html"
  ${nginxSiteStatic} "static.localhost" "$STATIC_ROOT"

  LIST_OUT="$PWD/nginx-sites.log"
  ${nginxSiteList} > "$LIST_OUT"
  grep -q "example.localhost" "$LIST_OUT" || fail "site list missing example.localhost"
  grep -q "static.localhost" "$LIST_OUT" || fail "site list missing static.localhost"

  ${nginxSiteDisable} "example.localhost"
  [ ! -e "$NGINX_DIR/conf/sites-enabled/example.localhost.conf" ] || fail "site-disable did not remove enabled link"

  ${nginxSiteEnable} "example.localhost"
  [ -e "$NGINX_DIR/conf/sites-enabled/example.localhost.conf" ] || fail "site-enable did not restore enabled link"

  ${nginxSiteRemove} "example.localhost"
  [ ! -e "$NGINX_DIR/conf/sites-available/example.localhost.conf" ] || fail "site-remove did not remove available config"

  set +e
  ${nginxSiteEnable} "missing.localhost" >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "site-enable should fail for missing site"

  set +e
  ${nginxSiteStatic} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "site-static should fail with missing args"

  set +e
  ${nginxStatus} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "nginxStatus should be non-zero when not running"

  set +e
  ${nginxHealth} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "nginxHealth should fail when not running"

  set +e
  ${nginxReady} >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "nginxReady should fail when not running"

  NGINX_PID=""
  NGINX_PID=$(start_service nginx -- ${nginxStart})
  READY=0
  for _ in $(seq 1 80); do
    if ${nginxReady} >/dev/null 2>&1; then
      READY=1
      break
    fi
    sleep 0.2
  done
  if [ "$READY" -ne 1 ]; then
    print_log_tail "$NGINX_DIR/logs/error.log" 50
    cleanup_nginx
    fail "nginx did not become ready"
  fi

  if ! wait_ok 20 0.2 ${nginxHealth}; then
    print_log_tail "$NGINX_DIR/logs/error.log" 50
    cleanup_nginx
    fail "nginxHealth should pass while running"
  fi

  if ! wait_ok 15 0.2 ${nginxStatus}; then
    cleanup_nginx
    fail "nginxStatus should pass while running"
  fi
  cleanup_nginx

  if ! wait_fail 40 0.2 ${nginxHealth}; then
    fail "nginxHealth should fail after stop"
  fi

  if ! wait_fail 20 0.2 ${nginxReady}; then
    fail "nginxReady should fail after stop"
  fi

  echo "nginx site lifecycle fixture ok"

''
