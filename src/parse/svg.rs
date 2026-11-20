use crate::math::Portable as _;
// The SVG profile reader for form tools: path extraction, tokenizing, flattening curves and arcs.

use super::*;

// ---- SVG profile parsing helpers -------------------------------------------

pub(super) fn extract_svg_paths(svg_content: &str) -> Result<String, String> {
    let mut merged_d = String::new();
    let mut input = svg_content;
    while let Some(path_start) = input.find("<path").or_else(|| input.find("<PATH")) {
        let rest = &input[path_start..];
        let path_end = rest.find('>').ok_or_else(|| "malformed SVG: path tag not closed".to_string())?;
        let tag_content = &rest[..path_end];
        
        let bytes = tag_content.as_bytes();
        let mut idx = 0;
        while idx < bytes.len() {
            if bytes[idx] == b'd' {
                let prev_ok = idx == 0 || bytes[idx - 1].is_ascii_whitespace();
                if prev_ok {
                    let mut temp = idx + 1;
                    while temp < bytes.len() && bytes[temp].is_ascii_whitespace() {
                        temp += 1;
                    }
                    if temp < bytes.len() && bytes[temp] == b'=' {
                        temp += 1;
                        while temp < bytes.len() && bytes[temp].is_ascii_whitespace() {
                            temp += 1;
                        }
                        if temp < bytes.len() {
                            let quote = bytes[temp];
                            if quote == b'"' || quote == b'\'' {
                                temp += 1;
                                let val_start = temp;
                                while temp < bytes.len() && bytes[temp] != quote {
                                    temp += 1;
                                }
                                if temp < bytes.len() {
                                    let d_val = std::str::from_utf8(&bytes[val_start..temp])
                                        .map_err(|e| format!("invalid UTF8 in SVG path data: {}", e))?;
                                    if !merged_d.is_empty() {
                                        merged_d.push(' ');
                                    }
                                    merged_d.push_str(d_val);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            idx += 1;
        }
        input = &rest[path_end + 1..];
    }
    if merged_d.is_empty() {
        return Err("no <path> with 'd' attribute found in SVG".to_string());
    }
    Ok(merged_d)
}

pub(super) fn tokenize_svg_d(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == ',' {
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() {
            tokens.push(c.to_string());
            i += 1;
            continue;
        }
        let mut num_str = String::new();
        let mut has_dot = false;
        let mut has_e = false;
        
        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
            num_str.push(chars[i]);
            i += 1;
        }
        
        while i < chars.len() {
            let next_c = chars[i];
            if next_c.is_ascii_digit() {
                num_str.push(next_c);
                i += 1;
            } else if next_c == '.' {
                if has_dot || has_e {
                    break;
                }
                has_dot = true;
                num_str.push(next_c);
                i += 1;
            } else if next_c == 'e' || next_c == 'E' {
                if has_e {
                    break;
                }
                has_e = true;
                num_str.push(next_c);
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    num_str.push(chars[i]);
                    i += 1;
                }
            } else if next_c == '+' || next_c == '-' {
                break;
            } else {
                break;
            }
        }
        if !num_str.is_empty() {
            tokens.push(num_str);
        } else {
            i += 1;
        }
    }
    tokens
}

pub(super) fn parse_svg_path_tokens(tokens: &[String], tol: f64) -> Result<Vec<Pt>, String> {
    let mut points = Vec::new();
    let mut i = 0;
    let mut curr = Pt { x: 0.0, y: 0.0 };
    let mut start_pt = Pt { x: 0.0, y: 0.0 };
    
    let mut last_cubic_control: Option<Pt> = None;
    let mut last_quad_control: Option<Pt> = None;
    
    enum LastCommand {
        Cubic,
        Quadratic,
        Other,
    }
    let mut last_cmd_type = LastCommand::Other;

    let get_num = |t: &str| -> Result<f64, String> {
        t.parse::<f64>().map_err(|_| format!("invalid number '{}' in SVG path", t))
    };

    let mut current_cmd = None;

    while i < tokens.len() {
        let tok = &tokens[i];
        let first_char = tok.chars().next().unwrap();
        
        let is_cmd = first_char.is_ascii_alphabetic();
        let cmd = if is_cmd {
            i += 1;
            first_char
        } else {
            current_cmd.ok_or_else(|| format!("expected SVG path command at token {}, found '{}'", i, tok))?
        };
        
        current_cmd = Some(cmd);

        match cmd {
            'M' | 'm' => {
                let x = get_num(&tokens[i])?;
                let y = get_num(&tokens[i+1])?;
                i += 2;
                
                let target = if cmd == 'm' {
                    Pt { x: curr.x + x, y: curr.y + y }
                } else {
                    Pt { x, y }
                };
                
                curr = target;
                start_pt = target;
                points.push(target);
                
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
                
                current_cmd = Some(if cmd == 'm' { 'l' } else { 'L' });
            }
            'L' | 'l' => {
                let x = get_num(&tokens[i])?;
                let y = get_num(&tokens[i+1])?;
                i += 2;
                
                let target = if cmd == 'l' {
                    Pt { x: curr.x + x, y: curr.y + y }
                } else {
                    Pt { x, y }
                };
                
                curr = target;
                points.push(target);
                
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
            }
            'H' | 'h' => {
                let x = get_num(&tokens[i])?;
                i += 1;
                
                let target = if cmd == 'h' {
                    Pt { x: curr.x + x, y: curr.y }
                } else {
                    Pt { x, y: curr.y }
                };
                
                curr = target;
                points.push(target);
                
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
            }
            'V' | 'v' => {
                let y = get_num(&tokens[i])?;
                i += 1;
                
                let target = if cmd == 'v' {
                    Pt { x: curr.x, y: curr.y + y }
                } else {
                    Pt { x: curr.x, y }
                };
                
                curr = target;
                points.push(target);
                
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
            }
            'C' | 'c' => {
                let x1 = get_num(&tokens[i])?;
                let y1 = get_num(&tokens[i+1])?;
                let x2 = get_num(&tokens[i+2])?;
                let y2 = get_num(&tokens[i+3])?;
                let x = get_num(&tokens[i+4])?;
                let y = get_num(&tokens[i+5])?;
                i += 6;
                
                let (p1, p2, p3) = if cmd == 'c' {
                    (
                        Pt { x: curr.x + x1, y: curr.y + y1 },
                        Pt { x: curr.x + x2, y: curr.y + y2 },
                        Pt { x: curr.x + x, y: curr.y + y },
                    )
                } else {
                    (Pt { x: x1, y: y1 }, Pt { x: x2, y: y2 }, Pt { x, y })
                };
                
                flatten_cubic_bezier(curr, p1, p2, p3, tol, 0, &mut points);
                curr = p3;
                last_cubic_control = Some(p2);
                last_quad_control = None;
                last_cmd_type = LastCommand::Cubic;
            }
            'S' | 's' => {
                let x2 = get_num(&tokens[i])?;
                let y2 = get_num(&tokens[i+1])?;
                let x = get_num(&tokens[i+2])?;
                let y = get_num(&tokens[i+3])?;
                i += 4;
                
                let p1 = match last_cmd_type {
                    LastCommand::Cubic => {
                        let prev_c = last_cubic_control.unwrap();
                        Pt { x: 2.0 * curr.x - prev_c.x, y: 2.0 * curr.y - prev_c.y }
                    }
                    _ => curr,
                };
                
                let (p2, p3) = if cmd == 's' {
                    (
                        Pt { x: curr.x + x2, y: curr.y + y2 },
                        Pt { x: curr.x + x, y: curr.y + y },
                    )
                } else {
                    (Pt { x: x2, y: y2 }, Pt { x, y })
                };
                
                flatten_cubic_bezier(curr, p1, p2, p3, tol, 0, &mut points);
                curr = p3;
                last_cubic_control = Some(p2);
                last_quad_control = None;
                last_cmd_type = LastCommand::Cubic;
            }
            'Q' | 'q' => {
                let x1 = get_num(&tokens[i])?;
                let y1 = get_num(&tokens[i+1])?;
                let x = get_num(&tokens[i+2])?;
                let y = get_num(&tokens[i+3])?;
                i += 4;
                
                let (p1, p2) = if cmd == 'q' {
                    (
                        Pt { x: curr.x + x1, y: curr.y + y1 },
                        Pt { x: curr.x + x, y: curr.y + y },
                    )
                } else {
                    (Pt { x: x1, y: y1 }, Pt { x, y })
                };
                
                flatten_quadratic_bezier(curr, p1, p2, tol, 0, &mut points);
                curr = p2;
                last_quad_control = Some(p1);
                last_cubic_control = None;
                last_cmd_type = LastCommand::Quadratic;
            }
            'T' | 't' => {
                let x = get_num(&tokens[i])?;
                let y = get_num(&tokens[i+1])?;
                i += 2;
                
                let p1 = match last_cmd_type {
                    LastCommand::Quadratic => {
                        let prev_c = last_quad_control.unwrap();
                        Pt { x: 2.0 * curr.x - prev_c.x, y: 2.0 * curr.y - prev_c.y }
                    }
                    _ => curr,
                };
                
                let p2 = if cmd == 't' {
                    Pt { x: curr.x + x, y: curr.y + y }
                } else {
                    Pt { x, y }
                };
                
                flatten_quadratic_bezier(curr, p1, p2, tol, 0, &mut points);
                curr = p2;
                last_quad_control = Some(p1);
                last_cubic_control = None;
                last_cmd_type = LastCommand::Quadratic;
            }
            'A' | 'a' => {
                let rx = get_num(&tokens[i])?;
                let ry = get_num(&tokens[i+1])?;
                let x_rot = get_num(&tokens[i+2])?;
                let large_arc = get_num(&tokens[i+3])? != 0.0;
                let sweep = get_num(&tokens[i+4])? != 0.0;
                let x = get_num(&tokens[i+5])?;
                let y = get_num(&tokens[i+6])?;
                i += 7;
                
                let target = if cmd == 'a' {
                    Pt { x: curr.x + x, y: curr.y + y }
                } else {
                    Pt { x, y }
                };
                
                let rx_abs = rx.abs();
                let ry_abs = ry.abs();
                if rx_abs < 1e-9 || ry_abs < 1e-9 {
                    points.push(target);
                } else {
                    let phi = x_rot.to_radians();
                    let f_a = if large_arc { 1 } else { 0 };
                    let f_s = if sweep { 1 } else { 0 };
                    
                    let cos_phi = phi.pcos();
                    let sin_phi = phi.psin();
                    
                    let dx2 = (curr.x - target.x) / 2.0;
                    let dy2 = (curr.y - target.y) / 2.0;
                    
                    let x1p = cos_phi * dx2 + sin_phi * dy2;
                    let y1p = -sin_phi * dx2 + cos_phi * dy2;
                    
                    let x1p_sq = x1p * x1p;
                    let y1p_sq = y1p * y1p;
                    
                    let mut rx_scaled = rx_abs;
                    let mut ry_scaled = ry_abs;
                    let lambda = x1p_sq / (rx_scaled * rx_scaled) + y1p_sq / (ry_scaled * ry_scaled);
                    if lambda > 1.0 {
                        let sqrt_lambda = lambda.sqrt();
                        rx_scaled *= sqrt_lambda;
                        ry_scaled *= sqrt_lambda;
                    }
                    
                    let rx_sq = rx_scaled * rx_scaled;
                    let ry_sq = ry_scaled * ry_scaled;
                    
                    let num = rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq;
                    let den = rx_sq * y1p_sq + ry_sq * x1p_sq;
                    
                    let factor = if den < 1e-9 {
                        0.0
                    } else {
                        (num / den).max(0.0).sqrt()
                    };
                    
                    let factor_signed = if f_a == f_s {
                        -factor
                    } else {
                        factor
                    };
                    
                    let cxp = factor_signed * (rx_scaled * y1p) / ry_scaled;
                    let cyp = -factor_signed * (ry_scaled * x1p) / rx_scaled;
                    
                    let cx = cos_phi * cxp - sin_phi * cyp + (curr.x + target.x) / 2.0;
                    let cy = sin_phi * cxp + cos_phi * cyp + (curr.y + target.y) / 2.0;
                    
                    let theta1 = ((y1p - cyp) / ry_scaled).patan2((x1p - cxp) / rx_scaled);
                    let theta2 = ((-y1p - cyp) / ry_scaled).patan2((-x1p - cxp) / rx_scaled);
                    
                    let mut d_theta = (theta2 - theta1) % (2.0 * std::f64::consts::PI);
                    if f_s == 0 && d_theta > 0.0 {
                        d_theta -= 2.0 * std::f64::consts::PI;
                    } else if f_s == 1 && d_theta < 0.0 {
                        d_theta += 2.0 * std::f64::consts::PI;
                    }
                    
                    flatten_arc(cx, cy, rx_scaled, ry_scaled, phi, theta1, theta1 + d_theta, tol, 0, &mut points);
                }
                
                curr = target;
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
            }
            'Z' | 'z' => {
                if (curr.x - start_pt.x).abs() > 1e-9 || (curr.y - start_pt.y).abs() > 1e-9 {
                    points.push(start_pt);
                }
                curr = start_pt;
                last_cubic_control = None;
                last_quad_control = None;
                last_cmd_type = LastCommand::Other;
                current_cmd = None;
            }
            _ => return Err(format!("unsupported SVG path command '{}'", cmd)),
        }
    }
    Ok(points)
}

pub(super) fn point_to_line_dist(q: Pt, a: Pt, b: Pt) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        let dqx = q.x - a.x;
        let dqy = q.y - a.y;
        (dqx * dqx + dqy * dqy).sqrt()
    } else {
        ((dx * (a.y - q.y) - (a.x - q.x) * dy).abs()) / len_sq.sqrt()
    }
}

pub(super) fn is_flat_cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, tol: f64) -> bool {
    point_to_line_dist(p1, p0, p3) <= tol && point_to_line_dist(p2, p0, p3) <= tol
}

pub(super) fn is_flat_quadratic(p0: Pt, p1: Pt, p2: Pt, tol: f64) -> bool {
    point_to_line_dist(p1, p0, p2) <= tol
}

pub(super) fn flatten_cubic_bezier(
    p0: Pt, p1: Pt, p2: Pt, p3: Pt,
    tol: f64,
    depth: u32,
    points: &mut Vec<Pt>,
) {
    if depth >= 15 || is_flat_cubic(p0, p1, p2, p3, tol) {
        points.push(p3);
    } else {
        let p01 = Pt { x: (p0.x + p1.x) / 2.0, y: (p0.y + p1.y) / 2.0 };
        let p12 = Pt { x: (p1.x + p2.x) / 2.0, y: (p1.y + p2.y) / 2.0 };
        let p23 = Pt { x: (p2.x + p3.x) / 2.0, y: (p2.y + p3.y) / 2.0 };
        let p012 = Pt { x: (p01.x + p12.x) / 2.0, y: (p01.y + p12.y) / 2.0 };
        let p123 = Pt { x: (p12.x + p23.x) / 2.0, y: (p12.y + p23.y) / 2.0 };
        let p0123 = Pt { x: (p012.x + p123.x) / 2.0, y: (p012.y + p123.y) / 2.0 };

        flatten_cubic_bezier(p0, p01, p012, p0123, tol, depth + 1, points);
        flatten_cubic_bezier(p0123, p123, p23, p3, tol, depth + 1, points);
    }
}

pub(super) fn flatten_quadratic_bezier(
    p0: Pt, p1: Pt, p2: Pt,
    tol: f64,
    depth: u32,
    points: &mut Vec<Pt>,
) {
    if depth >= 15 || is_flat_quadratic(p0, p1, p2, tol) {
        points.push(p2);
    } else {
        let p01 = Pt { x: (p0.x + p1.x) / 2.0, y: (p0.y + p1.y) / 2.0 };
        let p12 = Pt { x: (p1.x + p2.x) / 2.0, y: (p1.y + p2.y) / 2.0 };
        let p012 = Pt { x: (p01.x + p12.x) / 2.0, y: (p01.y + p12.y) / 2.0 };

        flatten_quadratic_bezier(p0, p01, p012, tol, depth + 1, points);
        flatten_quadratic_bezier(p012, p12, p2, tol, depth + 1, points);
    }
}

pub(super) fn flatten_arc(
    cx: f64, cy: f64, rx: f64, ry: f64, phi: f64,
    theta_a: f64, theta_b: f64,
    tol: f64,
    depth: u32,
    points: &mut Vec<Pt>,
) {
    let p_a = eval_arc_point(cx, cy, rx, ry, phi, theta_a);
    let p_b = eval_arc_point(cx, cy, rx, ry, phi, theta_b);
    let theta_m = (theta_a + theta_b) / 2.0;
    let p_m = eval_arc_point(cx, cy, rx, ry, phi, theta_m);
    
    if depth >= 15 || point_to_line_dist(p_m, p_a, p_b) <= tol {
        points.push(p_b);
    } else {
        flatten_arc(cx, cy, rx, ry, phi, theta_a, theta_m, tol, depth + 1, points);
        flatten_arc(cx, cy, rx, ry, phi, theta_m, theta_b, tol, depth + 1, points);
    }
}

pub(super) fn eval_arc_point(cx: f64, cy: f64, rx: f64, ry: f64, phi: f64, theta: f64) -> Pt {
    let cos_phi = phi.pcos();
    let sin_phi = phi.psin();
    let cos_t = theta.pcos();
    let sin_t = theta.psin();
    Pt {
        x: cx + rx * cos_t * cos_phi - ry * sin_t * sin_phi,
        y: cy + rx * cos_t * sin_phi + ry * sin_t * cos_phi,
    }
}
