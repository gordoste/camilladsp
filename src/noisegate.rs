use crate::audiodevice::AudioChunk;
use crate::config;
use crate::filters::Processor;
use crate::PrcFmt;
use crate::Res;

#[derive(Clone, Debug)]
pub struct NoiseGate {
    pub name: String,
    pub channels: usize,
    pub monitor_channels: Vec<usize>,
    pub process_channels: Vec<usize>,
    pub attack: PrcFmt,
    pub release: PrcFmt,
    pub threshold: PrcFmt,
    pub factor: PrcFmt,
    pub samplerate: usize,
    pub scratch: Vec<PrcFmt>,
    pub prev_loudness: PrcFmt,
}

impl NoiseGate {
    /// Creates a NoiseGate from a config struct
    pub fn from_config(
        name: &str,
        config: config::NoiseGateParameters,
        samplerate: usize,
        chunksize: usize,
    ) -> Self {
        let name = name.to_string();
        let channels = config.channels;
        let srate = samplerate as PrcFmt;
        let mut monitor_channels = config.monitor_channels();
        if monitor_channels.is_empty() {
            for n in 0..channels {
                monitor_channels.push(n);
            }
        }
        let mut process_channels = config.process_channels();
        if process_channels.is_empty() {
            for n in 0..channels {
                process_channels.push(n);
            }
        }
        let attack = (-1.0 / srate / config.attack).exp();
        let release = (-1.0 / srate / config.release).exp();
        let scratch = vec![0.0; chunksize];

        debug!("Creating noisegate '{}', channels: {}, monitor_channels: {:?}, process_channels: {:?}, attack: {}, release: {}, threshold: {}, attenuation: {}", 
                name, channels, process_channels, monitor_channels, attack, release, config.threshold, config.attenuation);

        let factor = (10.0 as PrcFmt).powf(-config.attenuation / 20.0);

        NoiseGate {
            name,
            channels,
            monitor_channels,
            process_channels,
            attack,
            release,
            threshold: config.threshold,
            factor,
            samplerate,
            scratch,
            prev_loudness: 0.0,
        }
    }

    /// Sum all channels that are included in loudness monitoring, store result in self.scratch
    fn sum_monitor_channels(&mut self, input: &AudioChunk) {
        let ch = self.monitor_channels[0];
        self.scratch.copy_from_slice(&input.waveforms[ch]);
        for ch in self.monitor_channels.iter().skip(1) {
            for (acc, val) in self.scratch.iter_mut().zip(input.waveforms[*ch].iter()) {
                *acc += *val;
            }
        }
    }

    /// Estimate loudness, store result in self.scratch
    fn estimate_loudness(&mut self) {
        for val in self.scratch.iter_mut() {
            // convert to dB
            *val = 20.0 * (val.abs() + 1.0e-9).log10();
            if *val >= self.prev_loudness {
                *val = self.attack * self.prev_loudness + (1.0 - self.attack) * *val;
            } else {
                *val = self.release * self.prev_loudness + (1.0 - self.release) * *val;
            }
            self.prev_loudness = *val;
        }
    }

    /// Calculate linear gain, store result in self.scratch
    fn calculate_linear_gain(&mut self) {
        for val in self.scratch.iter_mut() {
            if *val < self.threshold {
                *val = self.factor;
            } else {
                *val = 1.0;
            }
        }
    }

    fn apply_gain(&self, input: &mut [PrcFmt]) {
        for (val, gain) in input.iter_mut().zip(self.scratch.iter()) {
            *val *= gain;
        }
    }
}

impl Processor for NoiseGate {
    fn name(&self) -> &str {
        &self.name
    }

    /// Apply a NoiseGate to an AudioChunk, modifying it in-place.
    fn process_chunk(&mut self, input: &mut AudioChunk) -> Res<()> {
        self.sum_monitor_channels(input);
        self.estimate_loudness();
        self.calculate_linear_gain();
        for ch in self.process_channels.iter() {
            self.apply_gain(&mut input.waveforms[*ch]);
        }
        Ok(())
    }

    fn update_parameters(&mut self, config: config::Processor) {
        if let config::Processor::NoiseGate {
            parameters: config, ..
        } = config
        {
            let channels = config.channels;
            let srate = self.samplerate as PrcFmt;
            let mut monitor_channels = config.monitor_channels();
            if monitor_channels.is_empty() {
                for n in 0..channels {
                    monitor_channels.push(n);
                }
            }
            let mut process_channels = config.process_channels();
            if process_channels.is_empty() {
                for n in 0..channels {
                    process_channels.push(n);
                }
            }
            let attack = (-1.0 / srate / config.attack).exp();
            let release = (-1.0 / srate / config.release).exp();

            self.monitor_channels = monitor_channels;
            self.process_channels = process_channels;
            self.attack = attack;
            self.release = release;
            self.threshold = config.threshold;
            self.factor = (10.0 as PrcFmt).powf(-config.attenuation / 20.0);

            debug!("Updated noise gate '{}', monitor_channels: {:?}, process_channels: {:?}, attack: {}, release: {}, threshold: {}, attenuation: {}", 
                self.name, self.process_channels, self.monitor_channels, attack, release, config.threshold, config.attenuation);
        } else {
            // This should never happen unless there is a bug somewhere else
            panic!("Invalid config change!");
        }
    }
}

/// Validate the noise gate config, to give a helpful message intead of a panic.
pub fn validate_noise_gate(config: &config::NoiseGateParameters) -> Res<()> {
    let channels = config.channels;
    if config.attack <= 0.0 {
        let msg = "Attack value must be larger than zero.";
        return Err(config::ConfigError::new(msg).into());
    }
    if config.release <= 0.0 {
        let msg = "Release value must be larger than zero.";
        return Err(config::ConfigError::new(msg).into());
    }
    for ch in config.monitor_channels().iter() {
        if *ch >= channels {
            let msg = format!(
                "Invalid monitor channel: {}, max is: {}.",
                *ch,
                channels - 1
            );
            return Err(config::ConfigError::new(&msg).into());
        }
    }
    for ch in config.process_channels().iter() {
        if *ch >= channels {
            let msg = format!(
                "Invalid channel to process: {}, max is: {}.",
                *ch,
                channels - 1
            );
            return Err(config::ConfigError::new(&msg).into());
        }
    }
    Ok(())
}
