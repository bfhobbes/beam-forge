; Beam Forge frame
; Start from: current position, job origin center
; Bounds: X115 Y177.5 to X185 Y222.5
; Frame: rubber band, low power 1% (M4 S10)
G21 G90
M4 S0
G0 X115 Y177.5
G1 X155 Y177.5 S10 F3000
G1 X185 Y207.5
G1 X175 Y222.5
G1 X115 Y197.5
G1 X115 Y177.5
G0 X150 Y200 S0
M5
