use super::flight::danger;
use super::*;

#[test]
fn costs_are_never_free() {
    assert_eq!(non_zero(0.2), 1);
    assert_eq!(non_zero(0.0), 1);
    assert_eq!(non_zero(-0.2), -1);
    assert_eq!(non_zero(2.5), 3);
    assert_eq!(non_zero(12_500.4), 12_500);
}

#[test]
fn flight_danger() {
    let f = FlightAbility {
        time_left: -1,
        dim: "d".into(),
        origin: Vec3::ZERO,
        radius: 10.0,
    };
    assert_eq!(danger("d", Vec3::new(1.0, 50.0, 0.0), &f), 0.0);
    assert_eq!(danger("d", Vec3::new(8.0, 0.0, 0.0), &f), 0.5);
    assert_eq!(danger("d", Vec3::new(11.0, 0.0, 0.0), &f), 1.0);
    assert_eq!(danger("other", Vec3::ZERO, &f), 1.0);
    let t = FlightAbility {
        time_left: 70,
        radius: -1.0,
        ..f
    };
    assert_eq!(danger("d", Vec3::ZERO, &t), 0.5);
}
