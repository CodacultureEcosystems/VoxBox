pub struct EnergyVad {
    threshold: f32,
}

impl EnergyVad {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    pub fn is_speech(&self, frame: &[i16]) -> bool {
        if frame.is_empty() {
            return false;
        }
        let sum: f64 = frame
            .iter()
            .map(|&s| {
                let s = s as f64 / 32768.0;
                s * s
            })
            .sum();
        let rms = (sum / frame.len() as f64).sqrt() as f32;
        rms > self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_not_speech() {
        let vad = EnergyVad::new(0.004);
        assert!(!vad.is_speech(&[0i16; 480]));
        assert!(!vad.is_speech(&[10i16; 480]));
    }

    #[test]
    fn loud_frame_is_speech() {
        let vad = EnergyVad::new(0.004);
        let frame: Vec<i16> = (0..480).map(|i| (i * 300) as i16).collect();
        assert!(vad.is_speech(&frame));
    }

    #[test]
    fn empty_frame_is_not_speech() {
        let vad = EnergyVad::new(0.004);
        assert!(!vad.is_speech(&[]));
    }
}
