# Independent Nix interpretation of the authored runtime placeholder grammar.
{ lib }:
value:
let
  length = builtins.stringLength value;
  slice = start: count: builtins.substring start count value;
  prefixes = [
    { prefix = "\${port"; kind = "port"; }
    { prefix = "\${host"; kind = "host"; }
    { prefix = "\${stateDir"; kind = "stateDir"; }
    { prefix = "\${secret"; kind = "secret"; }
  ];
  recognize = text:
    let
      prefix = lib.findFirst (entry: lib.hasPrefix entry.prefix text) null prefixes;
    in if prefix == null then null else
    let
      inherit (prefix) kind;
      prefixLength = builtins.stringLength prefix.prefix;
      rest = builtins.substring prefixLength (-1) text;
      named = lib.hasPrefix ":" rest && kind != "stateDir";
      payload = builtins.substring 1 (-1) rest;
      parts = lib.splitString "}" payload;
      closed = builtins.length parts > 1;
      name = builtins.head parts;
      malformed = empty: { token = { malformed = kind; inherit empty; }; consumed = 2; };
    in
      if lib.hasPrefix "}" rest && kind != "secret" then
        { token = { ${kind} = null; }; consumed = prefixLength + 1; }
      else if named then
        if closed && name != "" && !(lib.hasInfix "{" name) then
          { token = { ${kind} = name; }; consumed = prefixLength + builtins.stringLength name + 2; }
        else malformed (closed && name == "")
      else if rest == "" && kind != "secret" then malformed false
      else null;
  scan = cursor: literal: tokens:
    if cursor >= length then
      tokens ++ lib.optional (literal < length) { literal = slice literal (length - literal); }
    else if slice cursor 2 != "\${" then scan (cursor + 1) literal tokens
    else let found = recognize (slice cursor (length - cursor)); in
      if found == null then scan (cursor + 2) literal tokens
      else let next = cursor + found.consumed; in
        scan next next (tokens
          ++ lib.optional (literal < cursor) { literal = slice literal (cursor - literal); }
          ++ [ found.token ]);
in
scan 0 0 [ ]
