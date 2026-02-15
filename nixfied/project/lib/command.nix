{ project }:

let
  hasPrefix =
    prefix: value:
    builtins.isString value
    && (builtins.stringLength value) >= (builtins.stringLength prefix)
    && (builtins.substring 0 (builtins.stringLength prefix) value) == prefix;

  tokenToArgName =
    token:
    let
      base0 = if hasPrefix "--" token then builtins.substring 2 ((builtins.stringLength token) - 2) token else token;
      base1 = if hasPrefix "-" base0 then builtins.substring 1 ((builtins.stringLength base0) - 1) base0 else base0;
      base2 =
        let
          equalMatch = builtins.match "^([^=]+)=.*$" base1;
        in
        if equalMatch == null then base1 else builtins.head equalMatch;
    in
    builtins.replaceStrings [ "-" "." ":" " " "/" ] [ "_" "_" "_" "_" "_" ] base2;
in
{
  mkEnvDocProjectEnv =
    defaultEnv: {
      name = project.envVar;
      description = "Environment name (set to ${defaultEnv} by default for this command)";
    };

  mkEnvDocSlot = {
    name = project.slotVar;
    description = "Slot number (0-9)";
  };

  mkPlaceholderScript =
    message: ''
      echo "${message}"
      exit 0
    '';

  mkProjectCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
    }:
    let
      argSpecs =
        map
          (
            arg:
            let
              token = arg.name or "";
              isLong = hasPrefix "--" token;
              isShort = (!isLong) && hasPrefix "-" token;
              kind =
                if isLong || isShort then
                  if (builtins.match "^--[^=]+=.+$" token) != null then "option" else "flag"
                else
                  "positional";
              type = if kind == "flag" then "bool" else "string";
            in
            {
              name = tokenToArgName token;
              inherit kind type;
            }
            // (if isLong then { long = token; } else { })
            // (if isShort then { short = token; } else { })
          )
          args;

      envSpecs = map (doc: {
        name = doc.name;
        type = "string";
        required = false;
      }) envDocs;
    in
    {
      inherit description env useDeps script;
      api =
        ({
          version = 2;
          inherit summary details usage examples category;
          appContract = {
            version = 2;
            inherit name;
            allowUnknownArgs = false;
            args = argSpecs;
            env = envSpecs;
            outputs = {
              mode = "text";
            };
            failureCodes = {
              generic = 1;
              usage = 2;
              precondition = 3;
              unavailable = 4;
              timeout = 5;
            };
            idempotent = true;
          };
        })
        // (if args != [ ] then { inherit args; } else { })
        // (if envDocs != [ ] then { env = envDocs; } else { });
    };
}
