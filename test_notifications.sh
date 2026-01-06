#!/bin/bash
# Test script for runst notification daemon

echo "Sending test notifications..."

# Test 1: Basic notification
notify-send "Test 1" "Simple notification body"
sleep 1

# Test 2: Multi-line body
notify-send "Test 2 - Multiline" "Line one
Line two
Line three"
sleep 1

# Test 3: Special characters that need escaping
notify-send "Test 3 - Special Chars" "Contains <angle> brackets & ampersand"
sleep 1

# Test 4: Quotes and apostrophes
notify-send "Test 4 - Quotes" "He said \"hello\" and it's working"
sleep 1

# Test 5: Path-like content
notify-send "Test 5 - Paths" "/home/user/some/path/file.txt"
sleep 1

# Test 6: Code-like content
notify-send "Test 6 - Code" "if (x < 10 && y > 5) { return true; }"
sleep 1

# Test 7: Long notification
notify-send "Test 7 - Long" "This is a much longer notification body that contains a lot of text to test how the window handles wrapping and sizing when there is significant content to display"
sleep 1

# Test 8: Unicode and emoji
notify-send "Test 8 - Unicode" "Hello world! Stars: ★★★ Arrows: → ← ↑ ↓"
sleep 1

# Test 9: Simulated Claude notification
notify-send "Claude Code [1.2]" "Notification:task_complete
Build succeeded with 0 errors
/home/dave/git/runst"
sleep 1

# Test 10: Simulated Bash notification
notify-send "Bash completed" "Command: cargo build --release
Exit code: 0"
sleep 1

# Test 11: Clickable notification 1 (RED)
echo "Sending clickable notification 1 (RED)..."
gdbus call --session \
    --dest org.freedesktop.Notifications \
    --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Notify \
    "Clickable RED" \
    'uint32 0' \
    "" \
    "Click RED" \
    "I am the RED notification - click me!" \
    "['default', 'Open Red']" \
    '{}' \
    'int32 -1'
sleep 1

# Test 12: Clickable notification 2 (GREEN)
echo "Sending clickable notification 2 (GREEN)..."
gdbus call --session \
    --dest org.freedesktop.Notifications \
    --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Notify \
    "Clickable GREEN" \
    'uint32 0' \
    "" \
    "Click GREEN" \
    "I am the GREEN notification - click me!" \
    "['default', 'Open Green']" \
    '{}' \
    'int32 -1'
sleep 1

# Test 13: Clickable notification 3 (BLUE)
echo "Sending clickable notification 3 (BLUE)..."
gdbus call --session \
    --dest org.freedesktop.Notifications \
    --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.Notify \
    "Clickable BLUE" \
    'uint32 0' \
    "" \
    "Click BLUE" \
    "I am the BLUE notification - click me!" \
    "['default', 'Open Blue']" \
    '{}' \
    'int32 -1'
sleep 1

echo "Done! Sent 13 test notifications (last 3 are clickable)."
