{ pkgs }:
let
  config = import ../../nixfied/modules/services/runtime/postgres/config.nix {
    inherit pkgs;
    project = { };
  };
  lifecycleSource = builtins.readFile ../../nixfied/modules/services/runtime/postgres/lifecycle.nix;
  devConfText = config.devConf;
  prodConfText = config.prodConf;
  testConfText = config.testConf;
  pgHbaConfText = config.pgHbaConf;
in
assert pkgs.lib.hasInfix "listen_addresses = 'localhost'" devConfText;
assert pkgs.lib.hasInfix "shared_memory_type = mmap" devConfText;
assert pkgs.lib.hasInfix "dynamic_shared_memory_type = mmap" devConfText;
assert pkgs.lib.hasInfix "max_connections = 20" devConfText;
assert pkgs.lib.hasInfix "shared_buffers = 32MB" devConfText;
assert pkgs.lib.hasInfix "fsync = off" devConfText;
assert pkgs.lib.hasInfix "max_connections = 50" prodConfText;
assert pkgs.lib.hasInfix "shared_buffers = 128MB" prodConfText;
assert pkgs.lib.hasInfix "archive_mode = on" prodConfText;
assert pkgs.lib.hasInfix "max_connections = 20" testConfText;
assert pkgs.lib.hasInfix "shared_buffers = 32MB" testConfText;
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
