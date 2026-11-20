%
(nacs — nacs output)
(tool T3 ChamferMill 90.000deg body dia 12.000 tip 0.500 -> comp register D3 = 1.500)
(  D3 is the cone diameter 0.750 above the virtual apex, not the body OD: the tip is a relieved flat 0.500 wide and nothing positions from it)
(tool T7 dia 6.000 -> comp register D7)
G21
G90 G54 G17
M06 T3
M03 S3000.000
M08
G00 G43 H3 Z25.000
(--- form points: Hole2 (spot 3.000) with T3 ---)
G00 Z25.000
G00 X60.000 Y40.000
G00 Z5.000
G01 Z-1.250 F300.000
G00 Z25.000
M09
M05
G00 Z25.000
M06 T7
M03 S3000.000
M08
G00 G43 H7 Z25.000
(--- form points: Hole1, Hole2 with T7 ---)
G00 Z25.000
G00 X30.000 Y40.000
G00 Z5.000
G01 Z-15.000 F300.000
G00 Z25.000
G00 X60.000 Y40.000
G00 Z5.000
G01 Z-15.000 F300.000
G00 Z25.000
M09
M05
G00 Z25.000
M06 T3
M03 S3000.000
M08
G00 G43 H3 Z25.000
(--- form points: Hole1 (csink 11.000) with T3 ---)
G00 Z25.000
G00 X30.000 Y40.000
G00 Z5.000
G01 Z-5.250 F300.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
