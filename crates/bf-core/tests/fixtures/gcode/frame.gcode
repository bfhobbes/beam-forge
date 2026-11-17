; Beam Forge frame
; Start from: absolute coordinates
; Bounds: X10 Y10 to X80 Y55
; Frame: bounding box, laser off
G21 G90
M5 S0
G0 X10 Y10
G1 X80 Y10 F3000
G1 X80 Y55
G1 X10 Y55
G1 X10 Y10
M5
