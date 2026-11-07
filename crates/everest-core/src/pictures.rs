//! Phase 3 pictures: convert an image file and upload it to a display key
//! or the dial, or reset pictures to the factory ones.
//!
//! [`convert`] turns PNG/JPEG/GIF/BMP into the keyboard's RGB565 format.
//! [`upload`] runs the Base Camp transfer sequence over any [`Transport`];
//! the CLI's dry run runs it against [`SimulatedKeyboard`], so the exact
//! packets are shown without opening a device. [`crate::Keyboard::upload_picture`]
//! is the real thing.
//!
//! The sequence and timing follow everest-mac (MIT), which follows Base
//! Camp; see "Pictures (phase 3)" in `docs/PROTOCOL.md`.

use std::path::Path;
use std::time::{Duration, Instant};

use everest_hid::{Session, Transport};
use everest_proto::pictures::{STATUS_BUSY, STATUS_OK, STATUS_REJECTED};
use everest_proto::{
    ImageChunk, Packet, Picture, PictureError, PictureTarget, TransferStatus, PACKET_SIZE,
};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, RgbImage, RgbaImage};
use serde::Serialize;

/// How an image is fitted to the target size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Scale to fill the target and crop the centre (keeps the aspect ratio).
    #[default]
    Cover,
    /// Scale to fit inside the target, black borders (keeps the aspect ratio).
    Contain,
    /// Scale to the target size exactly (distorts other aspect ratios).
    Stretch,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("cannot read image {path}: {source}")]
    Image {
        path: String,
        source: image::ImageError,
    },
    #[error(transparent)]
    Picture(#[from] PictureError),
}

/// Flattens transparency onto black (the displays have no alpha).
fn flatten(img: &RgbaImage) -> RgbImage {
    RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let [r, g, b, a] = img.get_pixel(x, y).0;
        let f = |v: u8| ((u16::from(v) * u16::from(a) + 127) / 255) as u8;
        image::Rgb([f(r), f(g), f(b)])
    })
}

/// Scales `img` to the target size with `fit`.
pub fn fit_image(img: &DynamicImage, width: u32, height: u32, fit: Fit) -> RgbImage {
    const FILTER: FilterType = FilterType::Lanczos3;
    let rgba = match fit {
        Fit::Stretch => img.resize_exact(width, height, FILTER).to_rgba8(),
        Fit::Cover => img.resize_to_fill(width, height, FILTER).to_rgba8(),
        Fit::Contain => {
            let scaled = img.resize(width, height, FILTER).to_rgba8();
            let mut canvas = RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
            let x = (width - scaled.width()) / 2;
            let y = (height - scaled.height()) / 2;
            image::imageops::overlay(&mut canvas, &scaled, i64::from(x), i64::from(y));
            canvas
        }
    };
    flatten(&rgba)
}

/// Converts a decoded image for `target`.
pub fn convert_image(
    img: &DynamicImage,
    target: PictureTarget,
    fit: Fit,
) -> Result<Picture, PictureError> {
    let rgb = fit_image(img, target.width(), target.height(), fit);
    Picture::from_rgb8(target, rgb.as_raw())
}

/// Reads an image file (PNG, JPEG, GIF: first frame, BMP) and converts it
/// for `target`. Returns the picture and the source size.
pub fn convert(
    path: &Path,
    target: PictureTarget,
    fit: Fit,
) -> Result<(Picture, (u32, u32)), ConvertError> {
    let err = |source| ConvertError::Image {
        path: path.display().to_string(),
        source,
    };
    let img = image::ImageReader::open(path)
        .map_err(|e| err(image::ImageError::IoError(e)))?
        .with_guessed_format()
        .map_err(|e| err(image::ImageError::IoError(e)))?
        .decode()
        .map_err(err)?;
    let size = img.dimensions();
    Ok((convert_image(&img, target, fit)?, size))
}

/// Writes the picture as the keyboard will show it (after RGB565
/// rounding), for checking before an upload. The format follows the
/// file extension (PNG recommended).
pub fn save_preview(picture: &Picture, path: &Path) -> Result<(), image::ImageError> {
    let t = picture.target();
    let img = RgbImage::from_raw(t.width(), t.height(), picture.to_rgb8())
        .expect("picture size matches its target");
    img.save(path)
}

/// Timing and retry limits of the transfer. The defaults follow
/// everest-mac / Base Camp; tests use [`TransferTiming::instant`].
#[derive(Clone, Copy, Debug, Serialize)]
pub struct TransferTiming {
    /// Wait between SET_REPORT and GET_REPORT for steps 1 and 2.
    #[serde(serialize_with = "millis")]
    pub select_wait: Duration,
    /// Extra pause before retrying steps 1 and 2.
    #[serde(serialize_with = "millis")]
    pub select_retry: Duration,
    pub select_attempts: u32,
    /// Wait after each descriptor before reading its answer, and again
    /// before re-sending it on `fb`.
    #[serde(serialize_with = "millis")]
    pub descriptor_wait: Duration,
    /// Give up on the descriptor after this long (nothing started yet).
    #[serde(serialize_with = "millis")]
    pub descriptor_timeout: Duration,
    /// Wait after each chunk before reading its answer.
    #[serde(serialize_with = "millis")]
    pub chunk_wait: Duration,
    /// Answers in a row that do not move the byte count before giving up.
    pub max_stalls: u32,
    /// Polls for the final "done" answer, and the pause between them.
    pub final_polls: u32,
    #[serde(serialize_with = "millis")]
    pub final_wait: Duration,
    /// How long to wait for `ff aa` after a display-key picture.
    #[serde(serialize_with = "millis")]
    pub refresh_wait: Duration,
}

fn millis<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_millis() as u64)
}

impl Default for TransferTiming {
    fn default() -> Self {
        TransferTiming {
            select_wait: Duration::from_millis(80),
            select_retry: Duration::from_millis(120),
            select_attempts: 40,
            descriptor_wait: Duration::from_millis(150),
            descriptor_timeout: Duration::from_secs(90),
            chunk_wait: Duration::from_millis(30),
            max_stalls: 600,
            final_polls: 40,
            final_wait: Duration::from_millis(100),
            refresh_wait: Duration::from_secs(1),
        }
    }
}

impl TransferTiming {
    /// No pauses at all (tests, dry run).
    pub fn instant() -> Self {
        TransferTiming {
            select_wait: Duration::ZERO,
            select_retry: Duration::ZERO,
            descriptor_wait: Duration::ZERO,
            descriptor_timeout: Duration::from_secs(5),
            chunk_wait: Duration::ZERO,
            final_wait: Duration::ZERO,
            refresh_wait: Duration::ZERO,
            ..Self::default()
        }
    }
}

/// Where a transfer stopped, if it did not complete.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Hid(#[from] everest_hid::Error),
    /// Steps 1-2 failed. Nothing was started; the keyboard is untouched.
    #[error("the keyboard did not accept the picture target ({step}); nothing was started")]
    TargetNotAccepted { step: &'static str },
    /// The descriptor was rejected (`fe`) or never accepted. Nothing was
    /// started; the keyboard is untouched.
    #[error("the keyboard {0} the picture; nothing was started")]
    DescriptorNotAccepted(&'static str),
    /// The transfer started but did not finish. The keyboard may wait for
    /// data until it is replugged.
    #[error(
        "the transfer stopped after {received} of {total} bytes ({why}). \
         Unplug and replug the keyboard; the rest of its flash is untouched"
    )]
    Interrupted {
        received: usize,
        total: usize,
        why: &'static str,
    },
}

impl TransferError {
    /// True when the keyboard had accepted the descriptor, i.e. a replug
    /// may be needed.
    pub fn started(&self) -> bool {
        matches!(self, TransferError::Interrupted { .. })
    }
}

/// What a completed transfer looked like.
#[derive(Clone, Debug, Serialize)]
pub struct TransferReport {
    pub target: PictureTarget,
    pub bytes: usize,
    pub checksum: u16,
    pub chunks_sent: usize,
    pub chunk_resends: usize,
    pub descriptor_sends: usize,
    /// The final answer carried the "done" mark.
    pub confirmed_done: bool,
    /// `ff aa` (display refreshed) was seen afterwards (display keys only).
    pub refresh_seen: bool,
    #[serde(serialize_with = "millis")]
    pub elapsed: Duration,
}

/// Progress callback: (bytes acknowledged, total).
pub type Progress<'a> = &'a mut dyn FnMut(usize, usize);

fn select_step<T: Transport>(
    session: &mut Session<T>,
    packet: &Packet,
    step: &'static str,
    timing: &TransferTiming,
) -> Result<(), TransferError> {
    let command = packet.as_bytes()[2];
    for _ in 0..timing.select_attempts.max(1) {
        if let Some(r) = session.feature_exchange(packet, timing.select_wait)? {
            if r.accepted(command) {
                return Ok(());
            }
        }
        std::thread::sleep(timing.select_retry);
    }
    Err(TransferError::TargetNotAccepted { step })
}

/// Runs the picture transfer (steps 1-6 of "Pictures (phase 3)").
///
/// The session must have [`everest_hid::Policy::Pictures`]. Before the
/// descriptor is accepted every failure leaves the keyboard untouched; after
/// that, the transfer is only given up when a write fails or the keyboard
/// stops making progress, and the error says a replug is needed.
pub fn upload<T: Transport>(
    session: &mut Session<T>,
    picture: &Picture,
    timing: &TransferTiming,
    progress: Progress<'_>,
) -> Result<TransferReport, TransferError> {
    let start = Instant::now();
    let target = picture.target();
    let total = picture.bytes().len();

    // Steps 1-2: select and confirm the target.
    select_step(
        session,
        &Packet::picture_select_target(target),
        "aa 55 21",
        timing,
    )?;
    select_step(
        session,
        &Packet::picture_confirm_target(),
        "aa 55 22",
        timing,
    )?;

    // Step 3: the descriptor, re-sent on every "busy" answer.
    let descriptor = Packet::picture_descriptor(picture);
    let mut descriptor_sends = 0;
    loop {
        descriptor_sends += 1;
        let reply = session.feature_exchange(&descriptor, timing.descriptor_wait)?;
        if session.transfer().is_some() {
            break;
        }
        if let Some(r) = reply.filter(|r| r.command == 0x10) {
            if r.status == TransferStatus::Rejected {
                return Err(TransferError::DescriptorNotAccepted("rejected"));
            }
        }
        if start.elapsed() >= timing.descriptor_timeout {
            return Err(TransferError::DescriptorNotAccepted("never accepted"));
        }
        std::thread::sleep(timing.descriptor_wait);
    }
    progress(0, total);

    // Step 4: chunks, following the keyboard's byte count.
    let mut chunks_sent = 0;
    let mut chunk_resends = 0;
    let mut stalls = 0;
    let mut confirmed_done = false;
    let mut last_offset = None;
    while let Some(armed) = session.transfer() {
        let offset = armed.received;
        let chunk: ImageChunk = picture
            .chunk_at(offset)
            .expect("an armed transfer has bytes left");
        if last_offset == Some(offset) {
            chunk_resends += 1;
        }
        last_offset = Some(offset);
        let reply = match session.send_chunk(&chunk, timing.chunk_wait) {
            Ok(r) => r,
            Err(everest_hid::Error::Hid(_)) => {
                return Err(TransferError::Interrupted {
                    received: offset,
                    total,
                    why: "a write failed; was the keyboard unplugged?",
                })
            }
            Err(e) => return Err(e.into()),
        };
        chunks_sent += 1;
        let now = session.transfer().map_or(total, |a| a.received);
        if let Some(r) = reply {
            confirmed_done |= r.confirms(total, picture.checksum());
        }
        if now > offset {
            stalls = 0;
            progress(now, total);
        } else {
            stalls += 1;
            if stalls > timing.max_stalls {
                return Err(TransferError::Interrupted {
                    received: offset,
                    total,
                    why: "the keyboard stopped acknowledging data",
                });
            }
        }
    }

    // Step 5: the final answer carries the "done" mark once the flash
    // commit is finished.
    let mut polls = 0;
    while !confirmed_done && polls < timing.final_polls {
        std::thread::sleep(timing.final_wait);
        if let Some(r) = session.poll_feature()? {
            confirmed_done = r.confirms(total, picture.checksum());
        }
        polls += 1;
    }

    // Step 6: display keys announce the refresh on the interrupt channel.
    let mut refresh_seen = false;
    if matches!(target, PictureTarget::Key { .. }) {
        session.send(&Packet::extended_info())?;
        refresh_seen = session.wait_for(&[0xFF, 0xAA], timing.refresh_wait)?;
    }

    Ok(TransferReport {
        target,
        bytes: total,
        checksum: picture.checksum(),
        chunks_sent,
        chunk_resends,
        descriptor_sends,
        confirmed_done,
        refresh_seen,
        elapsed: start.elapsed(),
    })
}

/// A keyboard in memory that answers the picture transfer like the real
/// one (as documented): accepts the two targets, answers `fb` to the first
/// `busy_descriptors` descriptors, then acknowledges chunks with a running
/// byte count. Records everything it was sent. Used by the dry run and the
/// tests; it never touches a device.
#[derive(Debug, Default)]
pub struct SimulatedKeyboard {
    /// Feature reports received, in order.
    pub features: Vec<[u8; PACKET_SIZE]>,
    /// Interrupt packets received, in order.
    pub written: Vec<[u8; PACKET_SIZE]>,
    /// Descriptors to answer with `fb` before accepting.
    pub busy_descriptors: usize,
    /// Chunk numbers (0-based count of chunk reports) to answer with `fb`.
    pub busy_chunks: Vec<usize>,
    /// Reject the descriptor (`fe`).
    pub reject: bool,
    /// Stop answering after this many chunk reports (simulates a hang).
    pub hang_after: Option<usize>,
    expected: Option<usize>,
    received: usize,
    chunk_reports: usize,
    reply: Option<[u8; PACKET_SIZE]>,
    inbox: std::collections::VecDeque<[u8; PACKET_SIZE]>,
    /// The picture bytes as the keyboard stored them.
    pub stored: Vec<u8>,
}

impl SimulatedKeyboard {
    fn answer(&mut self, prefix: &[u8]) {
        let mut r = [0u8; PACKET_SIZE];
        r[..prefix.len()].copy_from_slice(prefix);
        self.reply = Some(r);
    }

    fn ack(&mut self, status: u8) {
        let n = self.received;
        let done = self.expected.is_some_and(|e| n >= e);
        let mut r = [0u8; PACKET_SIZE];
        r[..7].copy_from_slice(&[
            0xAA,
            0x55,
            0x10,
            status,
            n as u8,
            (n >> 8) as u8,
            (n >> 16) as u8,
        ]);
        if done {
            // As firmware 57 answers: size, checksum echo, then fa.
            let [c0, c1] = everest_proto::pictures::checksum(&self.stored).to_le_bytes();
            r[7] = c0;
            r[8] = c1;
            r[9] = STATUS_OK;
        }
        self.reply = Some(r);
    }

    /// Feature reports that were picture chunks (after the descriptor).
    pub fn chunk_reports(&self) -> usize {
        self.chunk_reports
    }
}

impl Transport for SimulatedKeyboard {
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> everest_hid::Result<()> {
        self.written.push(*packet);
        if packet[..2] == [0x11, 0x14] && self.expected.is_some() {
            let mut r = [0u8; PACKET_SIZE];
            r[..2].copy_from_slice(&[0xFF, 0xAA]);
            self.inbox.push_back(r);
        }
        Ok(())
    }

    fn read_raw(&mut self, _timeout: Duration) -> everest_hid::Result<Option<[u8; PACKET_SIZE]>> {
        Ok(self.inbox.pop_front())
    }

    fn send_feature_raw(&mut self, p: &[u8; PACKET_SIZE]) -> everest_hid::Result<()> {
        self.features.push(*p);
        let receiving = self.expected.is_some_and(|e| self.received < e);
        if receiving {
            if self.hang_after.is_some_and(|n| self.chunk_reports >= n) {
                self.chunk_reports += 1;
                self.reply = None;
                return Ok(());
            }
            let busy = self.busy_chunks.contains(&self.chunk_reports);
            self.chunk_reports += 1;
            if busy {
                self.ack(STATUS_BUSY);
            } else {
                let left = self.expected.unwrap() - self.received;
                let n = left.min(PACKET_SIZE);
                self.stored.extend_from_slice(&p[..n]);
                self.received += n;
                self.ack(STATUS_OK);
            }
            return Ok(());
        }
        match (p[0], p[1], p[2]) {
            (0xAA, 0x55, 0x21) | (0xAA, 0x55, 0x22) => self.answer(&[0xAA, 0x55, p[2], STATUS_OK]),
            (0xAA, 0x55, 0x10) => {
                if self.reject {
                    self.answer(&[0xAA, 0x55, 0x10, STATUS_REJECTED]);
                } else if self.busy_descriptors > 0 {
                    self.busy_descriptors -= 1;
                    self.answer(&[0xAA, 0x55, 0x10, STATUS_BUSY]);
                } else {
                    let size = usize::from(p[3]) | usize::from(p[4]) << 8 | usize::from(p[5]) << 16;
                    self.expected = Some(size);
                    self.received = 0;
                    self.ack(STATUS_OK);
                }
            }
            _ => self.reply = None,
        }
        Ok(())
    }

    fn get_feature_raw(&mut self) -> everest_hid::Result<Option<[u8; PACKET_SIZE]>> {
        Ok(self.reply)
    }
}

#[cfg(test)]
mod tests {
    use everest_hid::Policy;
    use everest_proto::pictures::{checksum, KEY_BYTES};
    use everest_proto::{classify, SafetyClass};

    use super::*;

    fn test_picture(target: PictureTarget) -> Picture {
        let bytes = (0..target.byte_len()).map(|i| (i * 7) as u8).collect();
        Picture::from_rgb565(target, bytes).unwrap()
    }

    fn run(
        sim: SimulatedKeyboard,
        pic: &Picture,
    ) -> (Result<TransferReport, TransferError>, SimulatedKeyboard) {
        let mut s = Session::new(sim, Policy::Pictures);
        let r = upload(&mut s, pic, &TransferTiming::instant(), &mut |_, _| {});
        (r, s.into_transport())
    }

    #[test]
    fn key_upload_sequence() {
        let pic = test_picture(PictureTarget::key(2, 3).unwrap());
        let sim = SimulatedKeyboard {
            busy_descriptors: 2,
            busy_chunks: vec![5],
            ..Default::default()
        };
        let (r, sim) = run(sim, &pic);
        let r = r.unwrap();
        assert!(r.confirmed_done);
        assert!(r.refresh_seen);
        assert_eq!(r.descriptor_sends, 3);
        assert_eq!(r.chunks_sent, 163); // 162 + one resend
        assert_eq!(r.chunk_resends, 1);
        assert_eq!(sim.stored, pic.bytes());
        assert_eq!(checksum(&sim.stored), pic.checksum());

        // The packets before the chunks are exactly the documented ones.
        let head: Vec<String> = sim.features[..5]
            .iter()
            .map(|p| crate::hex(crate::lighting::trimmed(p)))
            .collect();
        let c = pic.checksum().to_le_bytes();
        let desc = format!("aa 55 10 80 28 00 {:02x} {:02x} 00 00 02 02 02", c[0], c[1]);
        assert_eq!(head, ["aa 55 21 04", "aa 55 22", &desc, &desc, &desc]);
        for p in &sim.features[..5] {
            assert_eq!(classify(p).class, SafetyClass::Transfer);
        }
        // After the transfer only the 11 14 read.
        assert_eq!(sim.written.len(), 1);
        assert_eq!(classify(&sim.written[0]).class, SafetyClass::Read);
    }

    #[test]
    fn dial_upload() {
        let pic = test_picture(PictureTarget::Dial);
        let (r, sim) = run(SimulatedKeyboard::default(), &pic);
        let r = r.unwrap();
        assert_eq!(r.chunks_sent, 1530);
        assert!(!r.refresh_seen);
        assert_eq!(sim.stored, pic.bytes());
        assert_eq!(sim.features[0][..4], [0xAA, 0x55, 0x21, 0x03]);
        assert_eq!(sim.features[2][..13][10..], [0x02, 0x01, 0x00]);
        assert!(sim.written.is_empty());
    }

    #[test]
    fn rejected_descriptor_starts_nothing() {
        let pic = test_picture(PictureTarget::key(1, 1).unwrap());
        let sim = SimulatedKeyboard {
            reject: true,
            ..Default::default()
        };
        let (r, sim) = run(sim, &pic);
        let e = r.unwrap_err();
        assert!(matches!(
            e,
            TransferError::DescriptorNotAccepted("rejected")
        ));
        assert!(!e.started());
        assert_eq!(sim.features.len(), 3);
        assert_eq!(sim.chunk_reports(), 0);
    }

    #[test]
    fn hang_is_reported_as_started() {
        let pic = test_picture(PictureTarget::key(1, 1).unwrap());
        let sim = SimulatedKeyboard {
            hang_after: Some(10),
            ..Default::default()
        };
        let (r, _) = run(sim, &pic);
        let e = r.unwrap_err();
        assert!(e.started(), "{e}");
        assert!(matches!(
            e,
            TransferError::Interrupted {
                received: 640,
                total: KEY_BYTES,
                ..
            }
        ));
    }

    #[test]
    fn other_policies_cannot_upload() {
        let pic = test_picture(PictureTarget::key(1, 1).unwrap());
        for policy in [Policy::ReadOnly, Policy::Lighting] {
            let mut s = Session::new(SimulatedKeyboard::default(), policy);
            let r = upload(&mut s, &pic, &TransferTiming::instant(), &mut |_, _| {});
            assert!(matches!(
                r,
                Err(TransferError::Hid(everest_hid::Error::Refused { .. }))
            ));
            assert!(s.into_transport().features.is_empty());
        }
    }

    #[test]
    fn fit_modes() {
        // 4 x 2 image: left half red, right half blue.
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(4, 2, |x, _| {
            if x < 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        }));
        // Lanczos rings a little at the edge, so compare loosely.
        let red = |p: [u8; 3]| p[0] > 200 && p[2] < 60;
        let blue = |p: [u8; 3]| p[2] > 200 && p[0] < 60;
        let contain = fit_image(&img, 8, 8, Fit::Contain);
        assert_eq!(contain.get_pixel(0, 0).0, [0, 0, 0]); // border above
        assert!(red(contain.get_pixel(0, 4).0));
        assert!(blue(contain.get_pixel(7, 4).0));
        // Cover keeps the middle 2 x 2 pixels: half red, half blue.
        let cover = fit_image(&img, 8, 8, Fit::Cover);
        assert_eq!(cover.dimensions(), (8, 8));
        assert!(red(cover.get_pixel(0, 0).0));
        assert!(blue(cover.get_pixel(7, 7).0));
        let stretch = fit_image(&img, 8, 8, Fit::Stretch);
        assert!(red(stretch.get_pixel(0, 0).0));
        assert!(blue(stretch.get_pixel(7, 7).0));
    }

    #[test]
    fn transparency_becomes_black() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            72,
            72,
            image::Rgba([255, 255, 255, 0]),
        ));
        let pic = convert_image(&img, PictureTarget::key(1, 1).unwrap(), Fit::Cover).unwrap();
        assert!(pic.bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn convert_file_and_preview() {
        let dir = std::env::temp_dir().join(format!("kebnekaise-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.png");
        RgbImage::from_pixel(100, 50, image::Rgb([0, 255, 0]))
            .save(&src)
            .unwrap();
        let (pic, size) = convert(&src, PictureTarget::Dial, Fit::Stretch).unwrap();
        assert_eq!(size, (100, 50));
        assert_eq!(&pic.bytes()[..2], &[0xE0, 0x07]);
        let preview = dir.join("preview.png");
        save_preview(&pic, &preview).unwrap();
        let back = image::open(&preview).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (240, 204));
        assert_eq!(back.get_pixel(10, 10).0, [0, 255, 0]);
        assert!(convert(&dir.join("missing.png"), PictureTarget::Dial, Fit::Cover).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
