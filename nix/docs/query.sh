# Native read-only dispatcher over private packaged presentation data.
usage() {
  printf '%s\n' "Usage: docs
       docs -h | --help
       docs options [prefix]
       docs option <exact-path>
       docs topic <name>
       docs api [kind]
       docs api <kind> <exact-id>
       docs source

Quote keyed option paths, for example:
  docs option 'nixfied.services.<name>.stateRefs'
  docs topic state
  docs topic placeholders
  docs api function library/compileModel"
}
fail() {
  printf 'docs: %s; use docs --help for query usage.\n' "$1" >&2
  exit 2
}
source_info() {
  jq -r '.source | to_entries[] | select(.value != null) | "\(.key): \(.value)"' "$index"
}
for argument in "$@"; do
  [[ -n "$argument" ]] || fail 'empty queries are not supported'
done
if [[ $# == 0 ]]; then
  printf 'Nixfied authoring and API reference\n\nSupplying framework source:\n'
  source_info
  printf '\nTopics:\n'
  jq -r '.topics | to_entries[] | "  \(.key): \(.value.section)"' "$index"
  printf '\n'
  usage
  exit 0
fi
case "$1" in
  -h|--help)
    [[ $# == 1 ]] || fail 'help accepts no operands'
    usage
    ;;
  source)
    [[ $# == 1 ]] || fail 'source accepts no operands'
    source_info
    ;;
  options)
    [[ $# -le 2 ]] || fail 'options accepts at most one namespace prefix'
    prefix="${2-}"
    if ! jq -er --arg prefix "$prefix" '
      [.options[].name | select($prefix == "" or . == $prefix or startswith($prefix + "."))]
      | sort | if length == 0 then empty else .[] end
    ' "$index"; then
      fail "unknown option namespace: $prefix"
    fi
    ;;
  option)
    [[ $# == 2 ]] || fail 'option requires one exact canonical path'
    if ! jq -er --arg name "$2" '.options[] | select(.name == $name) | .text' "$index"; then
      fail "unknown option: $2 (use docs options to list canonical paths)"
    fi
    ;;
  topic)
    [[ $# == 2 ]] || fail 'topic requires one exact name'
    if ! jq -er --arg name "$2" '
      if .topics[$name] then .topics[$name] as $topic
      | "Topic: \($name) — \($topic.section)\n\n", .documents[$topic.document]
      else empty end
    ' "$index"; then
      fail "unknown topic: $2 (use docs to list topics)"
    fi
    ;;
  api)
    [[ $# -le 3 ]] || fail 'api accepts a kind and an optional exact ID'
    if [[ $# == 1 ]]; then
      jq -r '[.api[].kind] | unique[]' "$index"
    elif [[ $# == 2 ]]; then
      if ! jq -er --arg kind "$2" '[.api[] | select(.kind == $kind) | .id] | sort | if length == 0 then empty else .[] end' "$index"; then
        fail "unknown API kind: $2 (use docs api to list kinds)"
      fi
    elif ! jq -er --arg kind "$2" --arg id "$3" '.api[] | select(.kind == $kind and .id == $id) | .text' "$index"; then
      fail "unknown API entry: $2 $3 (use docs api <kind> to list IDs)"
    fi
    ;;
  *) fail "unknown query: $1" ;;
esac
