# Seed a username, watch accounts stream in, pivot and go back, the timeline.
Output pivot.gif
Set Shell "bash"
Set FontSize 14
Set Width 1440
Set Height 900
Set Padding 12
Set TypingSpeed 35ms
Set Theme { "name": "spektr", "black": "#0d0e10", "red": "#e5766d", "green": "#8fcf8f", "yellow": "#e3b467", "blue": "#8ec5ff", "magenta": "#c6a0f6", "cyan": "#7fd1c7", "white": "#d8d5ce", "brightBlack": "#4a4d53", "brightRed": "#e5766d", "brightGreen": "#8fcf8f", "brightYellow": "#e3b467", "brightBlue": "#8ec5ff", "brightMagenta": "#c6a0f6", "brightCyan": "#7fd1c7", "brightWhite": "#ffffff", "background": "#0d0e10", "foreground": "#d8d5ce", "selection": "#1b2028", "cursor": "#d8d5ce" }
Require spektr

Hide
Type "spektr new tl-ctf-1101 --profile tracelabs --subject 'Jordan Marlow' --last-seen 2026-09-28 && clear" Enter
Sleep 1s
Show
Type "spektr open tl-ctf-1101.spektr" Sleep 300ms Enter
Sleep 2s
Type ":" Sleep 400ms Type "seed username:jmarlow.wav" Sleep 600ms Enter
Sleep 6s
Type "l" Sleep 300ms Type "l" Sleep 300ms Down Sleep 500ms Down Sleep 800ms
Enter Sleep 2500ms
Backspace Sleep 2s
Type "2" Sleep 3s
Type "G" Sleep 1500ms
Type "1" Sleep 1s
Type "q" Sleep 500ms
