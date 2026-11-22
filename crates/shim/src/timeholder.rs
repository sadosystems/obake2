//! Port of libTAS `TimeHolder`: a `timespec` with arithmetic.

use crate::real;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeHolder {
    pub sec: i64,
    pub nsec: i64,
}

pub const NSEC: i64 = 1_000_000_000;

impl TimeHolder {
    pub const ZERO: TimeHolder = TimeHolder { sec: 0, nsec: 0 };

    pub const fn new(sec: i64, nsec: i64) -> Self {
        Self { sec, nsec }
    }

    pub fn from_ts(ts: &libc::timespec) -> Self {
        let mut t = Self::new(ts.tv_sec, ts.tv_nsec);
        t.normalize();
        t
    }

    pub fn to_ts(self) -> libc::timespec {
        libc::timespec { tv_sec: self.sec, tv_nsec: self.nsec }
    }

    /// Real monotonic time (libTAS `TimeHolder::now`, a NATIVECALL).
    pub fn now() -> Self {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { real::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        Self::from_ts(&ts)
    }

    pub fn normalize(&mut self) {
        if self.nsec < 0 {
            let sec = (-self.nsec) / NSEC + 1;
            self.nsec += NSEC * sec;
            self.sec -= sec;
        }
        if self.nsec >= NSEC {
            let sec = self.nsec / NSEC;
            self.nsec -= NSEC * sec;
            self.sec += sec;
        }
    }

    pub fn mul(self, m: u32) -> Self {
        let total = (self.sec as i128 * NSEC as i128 + self.nsec as i128) * m as i128;
        Self::new((total / NSEC as i128) as i64, (total % NSEC as i128) as i64)
    }
}

impl std::ops::Add for TimeHolder {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        let mut t = Self::new(self.sec + o.sec, self.nsec + o.nsec);
        t.normalize();
        t
    }
}

impl std::ops::Sub for TimeHolder {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        let mut t = Self::new(self.sec - o.sec, self.nsec - o.nsec);
        t.normalize();
        t
    }
}

impl std::ops::AddAssign for TimeHolder {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl std::ops::SubAssign for TimeHolder {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_normalizes() {
        let a = TimeHolder::new(1, 900_000_000);
        let b = TimeHolder::new(0, 200_000_000);
        assert_eq!(a + b, TimeHolder::new(2, 100_000_000));
        assert_eq!(b - a, TimeHolder::new(-2, 300_000_000));
        assert_eq!(TimeHolder::new(0, 16_666_666).mul(3), TimeHolder::new(0, 49_999_998));
    }
}
