; Absolute to the start, then relative (G91) moves
G21 G90
G0 X50 Y50
G91
M4 S0
G1 X100 S500 F1500
G1 Y80
G1 X-100
G1 Y-80
G90
M5
