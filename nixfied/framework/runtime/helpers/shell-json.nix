# Shared shell JSON helpers for runtime-generated scripts.
{
}
:
''
  json_quote_string() {
    local value="$1"

    value="''${value//\\\\/\\\\\\\\}"
    value="''${value//\"/\\\"}"
    value="''${value//$'\n'/\\n}"
    value="''${value//$'\r'/\\r}"
    value="''${value//$'\t'/\\t}"
    value="''${value//$'\f'/\\f}"
    value="''${value//$'\b'/\\b}"

    printf '"'"'%s'"'"' "$value"
  }

  json_string_or_null() {
    local value="$1"

    if [ -z "$value" ]; then
      printf "null"
      return 0
    fi

    json_quote_string "$value"
  }
''
