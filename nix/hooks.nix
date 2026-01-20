# Module hook registry (optional)
{
  pkgs,
  project,
  slots,
  postgres ? null,
  nginx ? null,
}:

let
  postgresEnv =
    if postgres == null then
      { }
    else
      {
        POSTGRES_INIT = toString postgres.init;
        POSTGRES_START = toString postgres.start;
        POSTGRES_STOP = toString postgres.stop;
        POSTGRES_SETUP_DB = toString postgres.setupDb;
        POSTGRES_FULL_START = toString postgres.fullStart;
        POSTGRES_FULL_START_TEST = toString postgres.fullStartTest;
      };

  nginxEnv =
    if nginx == null then
      { }
    else
      {
        NGINX_INIT = toString nginx.init;
        NGINX_START = toString nginx.start;
        NGINX_STOP = toString nginx.stop;
        NGINX_SITE_PROXY = toString nginx.writeProxySite;
        NGINX_SITE_STATIC = toString nginx.writeStaticSite;
      };
in
{
  env =
    {
      SLOT_INFO = toString slots.getSlotInfo;
      REQUIRE_SLOT_ENV = toString slots.requireSlotEnv;
    }
    // postgresEnv
    // nginxEnv;
}
