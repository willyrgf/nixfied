{ pkgs }:
let
  config = import ../../nixfied/framework/runtime/services/postgres/config.nix {
    inherit pkgs;
    project = { };
  };
  lifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/lifecycle.nix;
  devConfText = config.devConf;
  prodConfText = config.prodConf;
  testConfText = config.testConf;
  pgHbaConfText = config.pgHbaConf;
in
assert pkgs.lib.hasInfix "listen_addresses = 'localhost'" devConfText;
assert pkgs.lib.hasInfix "fsync = off" devConfText;
assert pkgs.lib.hasInfix "archive_mode = on" prodConfText;
assert pkgs.lib.hasInfix "autovacuum = off" testConfText;
assert pkgs.lib.hasInfix "127.0.0.1/32  trust" pgHbaConfText;
assert pkgs.lib.hasInfix "select_config_template() {" lifecycleSource;
assert pkgs.lib.hasInfix "install -m 600 \"$PGCONF_TEMPLATE\" \"$PGDATA/postgresql.conf\""
  lifecycleSource;
assert pkgs.lib.hasInfix "config.pgHbaConfFile" lifecycleSource;
assert pkgs.lib.hasInfix "\"$PGDATA/pg_hba.conf\"" lifecycleSource;
assert pkgs.lib.hasInfix "ensure_config_port \"$PGDATA/postgresql.conf\"" lifecycleSource;
assert (!pkgs.lib.hasInfix "cat > \"$PGDATA/postgresql.conf\" <<'PGCONF'" lifecycleSource);
assert (!pkgs.lib.hasInfix "cat > \"$PGDATA/pg_hba.conf\" <<'EOF'" lifecycleSource);
pkgs.runCommand "postgres-config-artifacts-contract" { } ''
  echo "OK: postgres config artifacts are compiled in Nix and installed into PGDATA" > "$out"
''
