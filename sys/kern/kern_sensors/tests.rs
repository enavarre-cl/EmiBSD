use super::*;
use crate::sys::sensors::{SENSOR_FANRPM, SENSOR_TEMP};

extern crate std;
use std::boxed::Box;

fn leak<T>(v: T) -> &'static T {
    Box::leak(Box::new(v))
}

fn sensor(t: SensorType) -> &'static Ksensor {
    let s = leak(Ksensor::new());
    s.r#type.set(t);
    s
}

#[test]
fn sensor_attach_numbers_per_type() {
    let dev = leak(Ksensordev::new());
    let t0 = sensor(SENSOR_TEMP);
    let t1 = sensor(SENSOR_TEMP);
    let f0 = sensor(SENSOR_FANRPM);
    let t2 = sensor(SENSOR_TEMP);
    sensor_attach(dev, t0);
    sensor_attach(dev, t1);
    sensor_attach(dev, f0);
    sensor_attach(dev, t2);
    assert_eq!(
        (t0.numt.get(), t1.numt.get(), f0.numt.get(), t2.numt.get()),
        (0, 1, 0, 2)
    );
    assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);
    assert_eq!(dev.maxnumt.get()[SENSOR_FANRPM as usize], 1);
    assert_eq!(dev.sensors_count.get(), 4);

    // a hole in the middle keeps maxnumt and is filled again
    sensor_detach(dev, t1);
    assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);
    let t1b = sensor(SENSOR_TEMP);
    sensor_attach(dev, t1b);
    assert_eq!(t1b.numt.get(), 1);
    assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 3);

    // the tail one lowers maxnumt
    sensor_detach(dev, t2);
    assert_eq!(dev.maxnumt.get()[SENSOR_TEMP as usize], 2);
    assert_eq!(dev.sensors_count.get(), 3);
}

#[test]
fn sensordev_numbers_get_find() {
    let a = leak(Ksensordev::new());
    let b = leak(Ksensordev::new());
    let c = leak(Ksensordev::new());
    sensordev_install(a);
    sensordev_install(b);
    sensordev_install(c);
    assert_eq!((a.num.get(), b.num.get(), c.num.get()), (0, 1, 2));

    let s = sensor(SENSOR_FANRPM);
    sensor_attach(c, s);
    assert!(ptr::eq(sensor_find(2, SENSOR_FANRPM, 0).unwrap(), s));
    assert_eq!(sensor_find(2, SENSOR_TEMP, 0).err(), Some(Errno::ENOENT));

    sensordev_deinstall(b);
    assert_eq!(sensordev_get(1).err(), Some(Errno::ENXIO));
    assert_eq!(sensordev_get(7).err(), Some(Errno::ENOENT));
    assert!(ptr::eq(sensordev_get(2).unwrap(), c));

    // the gap is reused
    let d = leak(Ksensordev::new());
    sensordev_install(d);
    assert_eq!(d.num.get(), 1);

    sensordev_deinstall(a);
    sensordev_deinstall(c);
    sensordev_deinstall(d);
    assert_eq!(sensordev_get(0).err(), Some(Errno::ENOENT));
}
