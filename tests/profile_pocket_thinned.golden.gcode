%
(nacs — nacs output)
(tool T5 dia 10.000 -> comp register D5)
G21
G90 G54 G17
M06 T5
M03 S4774.648
M08
G00 G43 H5 Z25.000
(--- pocket clear: PocketProfile with T5 [chip thinning x1.250 at 53.1° engagement; per pass: x1.000 on the slot, then x1.250..x1.667 over 9 passes] ---)
(direction: climb — the material on the tool's right under M03)
G00 Z25.000
G00 X29.000 Y29.000
G00 Z5.000
(plunge: this loop is 8.000mm round, under one diameter — no room to ramp)
G01 Z-5.000 F300.000
G01 X31.000 Y29.000 F572.958
G01 X31.000 Y31.000
G01 X29.000 Y31.000
G01 X29.000 Y29.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X28.000 Y28.000
G01 Z-5.000 F300.000
G01 X32.000 Y28.000 F716.197
G01 X32.000 Y32.000
G01 X28.000 Y32.000
G01 X28.000 Y28.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X27.000 Y27.000
G01 Z-5.000 F300.000
G01 X33.000 Y27.000 F901.033
G01 X33.000 Y33.000
G01 X27.000 Y33.000
G01 X27.000 Y27.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X26.000 Y26.000
G01 Z-5.000 F300.000
G01 X34.000 Y26.000 F954.930
G01 X34.000 Y34.000
G01 X26.000 Y34.000
G01 X26.000 Y26.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X25.000 Y25.000
G01 Z-5.000 F300.000
G01 X35.000 Y25.000 F954.930
G01 X35.000 Y35.000
G01 X25.000 Y35.000
G01 X25.000 Y25.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X24.000 Y24.000
G01 Z-5.000 F300.000
G01 X36.000 Y24.000 F954.930
G01 X36.000 Y36.000
G01 X24.000 Y36.000
G01 X24.000 Y24.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X23.000 Y23.000
G01 Z-5.000 F300.000
G01 X37.000 Y23.000 F954.930
G01 X37.000 Y37.000
G01 X23.000 Y37.000
G01 X23.000 Y23.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X22.000 Y22.000
G01 Z-5.000 F300.000
G01 X38.000 Y22.000 F954.930
G01 X38.000 Y38.000
G01 X22.000 Y38.000
G01 X22.000 Y22.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X21.000 Y21.000
G01 Z-5.000 F300.000
G01 X39.000 Y21.000 F954.930
G01 X39.000 Y39.000
G01 X21.000 Y39.000
G01 X21.000 Y21.000
G00 Z5.000
(plunge: beside air an earlier loop cut, 1.000mm off its path)
G00 X20.000 Y20.000
G01 Z-5.000 F300.000
G01 X40.000 Y20.000 F954.930
G01 X40.000 Y40.000
G01 X20.000 Y40.000
G01 X20.000 Y20.000
G00 Z5.000
G00 Z25.000
M09
M05
G00 Z25.000
M30
%
