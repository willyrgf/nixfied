{ lib }:
let
  # JSON and Rust share ordinary escapes, but Rust spells control-code escapes
  # with braces. Split complete escapes so literal backslashes stay literal.
  quote =
    value:
    lib.concatStrings (
      map (
        part:
        if !builtins.isList part then
          part
        else
          let
            escape = builtins.head part;
          in
          if lib.hasPrefix "\\u" escape then
            "\\u{${builtins.substring 2 4 escape}}"
          else
            lib.replaceStrings [ "\\b" "\\f" ] [ "\\u{8}" "\\u{c}" ] escape
      ) (builtins.split "(\\\\u[0-9a-fA-F]{4}|\\\\.)" (builtins.toJSON value))
    );
in
quote
