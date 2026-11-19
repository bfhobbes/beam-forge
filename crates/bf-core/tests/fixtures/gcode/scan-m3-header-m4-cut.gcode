; LightBurn-style header: M3 S0 with no motion, then dynamic power (M4) cuts
G21 G90
M3 S0
G0 X20 Y20
M4
G1 X60 Y20 S700 F1200
G1 X60 Y60
M5
