# cc-uplink human grant affordances.  Source from ~/.tmux.conf:
#   source-file /path/to/uplink.tmux        (or: cc-uplink tmux-snippet > ~/.config/tmux/uplink.tmux)
# Requires tmux >= 3.0 (display-menu).
#
# Grants are HUMAN-only by design: the cc-uplink driver reads these pane
# options and never writes them. prefix+g acts on the focused pane — works
# even when the pane runs nc/telnet with no shell to type into.

bind-key g display-menu -T "#[align=centre]uplink grant — #{pane_id}#{?#{@name}, (#{@name}),}" \
  "observer (pin read-only)" o "set-option -p @uplink_profile observer" \
  "operator (interact)"      p "set-option -p @uplink_profile operator" \
  "godmode (breakglass)"     G "set-option -p @uplink_profile godmode" \
  "revoke grant"             r "set-option -pu @uplink_profile" \
  "" \
  "block read"               x "set-option -p @uplink_read off" \
  "allow read"               X "set-option -pu @uplink_read"

# Visibility is the audit surface: a grant you didn't make is conspicuous.
set -g pane-border-status top
set -g pane-border-format " #{?#{@uplink_profile},#[fg=yellow bold][#{@uplink_profile}]#[default] ,}#{?#{==:#{@uplink_read},off},#[fg=red][no-read]#[default] ,}#{pane_index}:#{pane_current_command} "
