# The avatar comparison in the inspector, v to confirm, c to capture.
Output capture.gif
Set Shell "bash"
Set FontSize 14
Set Width 1440
Set Height 900
Set Padding 12
Set TypingSpeed 35ms
Set Theme { "name": "spektr", "black": "#0d0e10", "red": "#e5766d", "green": "#8fcf8f", "yellow": "#e3b467", "blue": "#8ec5ff", "magenta": "#c6a0f6", "cyan": "#7fd1c7", "white": "#d8d5ce", "brightBlack": "#4a4d53", "brightRed": "#e5766d", "brightGreen": "#8fcf8f", "brightYellow": "#e3b467", "brightBlue": "#8ec5ff", "brightMagenta": "#c6a0f6", "brightCyan": "#7fd1c7", "brightWhite": "#ffffff", "background": "#0d0e10", "foreground": "#d8d5ce", "selection": "#1b2028", "cursor": "#d8d5ce" }
Require spektr

Hide
Type "spektr new tl-ctf-1101 --profile tracelabs --subject 'Jordan Marlow' --last-seen 2026-09-28 && spektr run tl-ctf-1101.spektr --seed username:jmarlow.wav --playbook person-from-username >/dev/null 2>&1 && clear" Enter
Sleep 8s
Show
Type "spektr open tl-ctf-1101.spektr" Sleep 300ms Enter
Sleep 3s
Type "h" Sleep 300ms Type "h" Sleep 500ms Down Sleep 2500ms
Type "v" Sleep 2500ms
Type "/" Type "jmarlow-wav" Enter Sleep 1500ms
Type "c" Sleep 1500ms Enter
Sleep 3s
Type "q" Sleep 500ms
