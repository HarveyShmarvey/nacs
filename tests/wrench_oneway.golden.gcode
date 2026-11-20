%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S3000.000
M08
G00 G43 H5 Z25.000
(--- floor: Top with T5 ---)
(open onto: loop 2)
(pattern: oneway along X, 13 passes at 4.992)
(direction: the rows cut conventional, the uncut floor on the tool's left under M03, every row the same way)
G00 Z25.000
(plunge: the first stroke is 0.010mm long, under one diameter — no room to ramp)
G00 X233.267 Y0.049
G00 Z5.000
G01 Z0.000 F300.000
G01 X233.277 Y0.049 F800.000
G00 Z5.000
(plunge: the first stroke is 1.376mm long, under one diameter — no room to ramp)
G00 X27.230 Y5.041
G00 Z5.000
G01 Z0.000 F300.000
G01 X28.606 Y5.041 F800.000
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X234.510 Y5.041
G00 Z5.000
G01 Z1.000 F300.000
G01 X215.429 Y5.041 Z0.000 F800.000
G01 X248.590 Y5.041
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X32.147 Y10.033
G00 Z5.000
G01 Z1.000 F300.000
G01 X13.066 Y10.033 Z0.000 F800.000
G01 X42.948 Y10.033
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X228.797 Y10.033
G00 Z5.000
G01 Z1.000 F300.000
G01 X209.715 Y10.033 Z0.000 F800.000
G01 X254.281 Y10.033
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X27.138 Y15.025
G00 Z5.000
G01 Z1.000 F300.000
G01 X8.057 Y15.025 Z0.000 F800.000
G01 X47.934 Y15.025
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X225.186 Y15.025
G00 Z5.000
G01 Z1.000 F300.000
G01 X206.105 Y15.025 Z0.000 F800.000
G01 X257.959 Y15.025
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X24.237 Y20.016
G00 Z5.000
G01 Z1.000 F300.000
G01 X5.156 Y20.016 Z0.000 F800.000
G01 X258.497 Y20.016
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X22.616 Y25.008
G00 Z5.000
G01 Z1.000 F300.000
G01 X3.535 Y25.008 Z0.000 F800.000
G01 X23.226 Y25.008
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X32.774 Y25.008
G00 Z5.000
G01 Z0.000 F300.000
G01 X229.149 Y25.008 F800.000
G00 Z5.000
(ramp: 2 legs along the first stroke, 18.090mm long, to hold 3.000deg)
G00 X3.005 Y30.000
G00 Z5.000
G01 Z1.000 F300.000
G01 X21.095 Y30.000 Z0.500 F800.000
G01 X3.005 Y30.000 Z0.000 F800.000
G01 X21.095 Y30.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X34.921 Y30.000
G00 Z5.000
G01 Z0.000 F300.000
G01 X227.041 Y30.000 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X3.535 Y34.992
G00 Z5.000
G01 Z0.000 F300.000
G01 X23.226 Y34.992 F800.000
G00 Z5.000
(ramp: down the first stroke, 19.081mm of it, to hold 3.000deg)
G00 X51.856 Y34.992
G00 Z5.000
G01 Z1.000 F300.000
G01 X32.774 Y34.992 Z0.000 F800.000
G01 X229.149 Y34.992
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X5.156 Y39.984
G00 Z5.000
G01 Z0.000 F300.000
G01 X258.497 Y39.984 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X8.057 Y44.975
G00 Z5.000
G01 Z0.000 F300.000
G01 X47.934 Y44.975 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X206.105 Y44.975
G00 Z5.000
G01 Z0.000 F300.000
G01 X257.959 Y44.975 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X13.066 Y49.967
G00 Z5.000
G01 Z0.000 F300.000
G01 X42.948 Y49.967 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X209.715 Y49.967
G00 Z5.000
G01 Z0.000 F300.000
G01 X254.281 Y49.967 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X27.230 Y54.959
G00 Z5.000
G01 Z0.000 F300.000
G01 X28.606 Y54.959 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X215.429 Y54.959
G00 Z5.000
G01 Z0.000 F300.000
G01 X248.590 Y54.959 F800.000
G00 Z5.000
(plunge: beside a stroke already cut, 4.992mm off it)
G00 X233.267 Y59.951
G00 Z5.000
G01 Z0.000 F300.000
G01 X233.277 Y59.951 F800.000
G00 Z5.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
