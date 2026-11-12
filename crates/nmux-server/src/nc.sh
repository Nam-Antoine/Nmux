#!/usr/bin/env bash
LC_ALL=C
session=$NMUX_SESSION
[ -n "$TMUX" ] && session=$(tmux display-message -p '#S' 2>/dev/null)
if [ -z "$session" ] || ! read -r port token pane proof 2>/dev/null <"$HOME/.nmux/panes/$session"; then
  while read -r other <&4; do
    [ "$other" -ef "${BASH_SOURCE[0]}" ] || exec "$other" "$@" 4<&-
  done 4< <(type -aP nc)
  echo "nc: this shell is not in an Nmux pane" >&2
  exit 1
fi
if ! exec 3<>"/dev/tcp/127.0.0.1/$port"; then
  echo "nc: cannot reach Nmux on your computer; restart this pane" >&2
  exit 1
fi
printf '%s\n' "$pane" >&3
read -r -t 15 shown <&3
if [ -z "$proof" ] || [ "$shown" != "$proof" ]; then
  echo "nc: what answers on this pane's link is not your Nmux; restart this pane" >&2
  exit 1
fi
tty=0
[ -t 1 ] && tty=1
input=0
for word; do
  [ "$word" = - ] && input=1
done
{
  printf '%s %s %s %s\n' "$token" "$tty" "$input" "$#"
  for word; do
    printf '%s\n%s' "${#word}" "$word"
  done
  if [ "$input" = 1 ]; then
    text=$(cat; printf .)
    text=${text%.}
    printf '%s\n%s' "${#text}" "$text"
  fi
} >&3
read -r code size <&3
if [ -z "$size" ]; then
  echo "nc: Nmux on your computer did not answer; restart this pane" >&2
  exit 1
fi
if [ "$size" -gt 0 ]; then
  IFS= read -r -N "$size" problem <&3
  printf '%s' "$problem" >&2
fi
cat <&3
exit "$code"
