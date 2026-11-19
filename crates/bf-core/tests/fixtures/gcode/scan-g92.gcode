; Sets the work origin to wherever the head is (G92), so the bounds depend
; on where it was left
G21 G90
G92 X0 Y0
M4 S0
G0 X10 Y10
G1 X50 Y10 S500 F1200
M5
