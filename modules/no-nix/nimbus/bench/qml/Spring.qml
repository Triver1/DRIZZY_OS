import QtQuick

// nimbus-style spring, parametrised like SwiftUI / nimbus `SpringCfg::new`:
// `response` ≈ seconds per oscillation, `zeta` = damping ratio.
//
// Qt's SpringAnimation integrates in fixed 16 ms steps with
//   v += spring·Δ − damping·v ;  x += v·0.016
// which is the discretisation of  x'' = (spring/h)·Δ − (damping/h)·x'  (h = 0.016).
// So spring = ω²·h and damping = 2ζω·h reproduce the nimbus oscillator.
SpringAnimation {
    property real response: 0.46
    property real zeta: 1.0
    readonly property real omega: 2 * Math.PI / response

    spring: omega * omega * 0.016
    damping: 2 * zeta * omega * 0.016
    epsilon: 0.002
}
