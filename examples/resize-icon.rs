// Asset tool: area-average resize (alpha-premultiplied, no halos) and
// silhouette recolor for VoxBox branding PNGs.
//
//   cargo run --example resize-icon -- IN.png OUT.png SIZE
//   cargo run --example resize-icon -- IN.png OUT.png 22
//   cargo run --example resize-icon -- IN.png OUT.png 0 --white   (recolor, no resize)

use std::env;
use std::fs::File;
use std::io::BufWriter;

fn decode(path: &str) -> (u32, u32, Vec<u8>) {
    let f = File::open(path).unwrap_or_else(|e| panic!("open {path}: {e}"));
    let dec = png::Decoder::new(std::io::BufReader::new(f));
    let mut r = dec.read_info().expect("read_info");
    let mut buf = vec![0u8; r.output_buffer_size().expect("size")];
    let info = r.next_frame(&mut buf).expect("decode");
    let (w, h) = (info.width, info.height);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let n = rgba.len();
    match info.color_type {
        png::ColorType::Rgba => rgba.copy_from_slice(&buf[..n]),
        png::ColorType::Rgb => {
            for i in 0..(w * h) as usize {
                let (r, g, b) = (buf[i * 3], buf[i * 3 + 1], buf[i * 3 + 2]);
                rgba[i * 4] = r;
                rgba[i * 4 + 1] = g;
                rgba[i * 4 + 2] = b;
                rgba[i * 4 + 3] = 255;
            }
        }
        ct => panic!("unsupported color type {ct:?}"),
    }
    (w, h, rgba)
}

fn encode(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let f = File::create(path).unwrap_or_else(|e| panic!("create {path}: {e}"));
    let bw = BufWriter::new(f);
    let mut enc = png::Encoder::new(bw, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("write_header");
    writer.write_image_data(rgba).expect("write_image_data");
}

/// Box-filter downscale/upscale in premultiplied-alpha space.
fn resize(w: u32, h: u32, src: &[u8], tw: u32, th: u32) -> Vec<u8> {
    assert!(tw > 0 && th > 0);
    let mut dst = vec![0u8; (tw * th * 4) as usize];
    for dy in 0..th {
        // Source row range covered by this destination row.
        let y0 = (dy as u64 * h as u64) / th as u64;
        let y1 = ((dy as u64 + 1) * h as u64) / th as u64;
        for dx in 0..tw {
            let x0 = (dx as u64 * w as u64) / tw as u64;
            let x1 = ((dx as u64 + 1) * w as u64) / tw as u64;
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    let i = ((y * w as u64 + x) * 4) as usize;
                    let a = src[i + 3] as u64;
                    acc[0] += src[i] as u64 * a;
                    acc[1] += src[i + 1] as u64 * a;
                    acc[2] += src[i + 2] as u64 * a;
                    acc[3] += a;
                    n += 1;
                }
            }
            let (r, g, b, a) = (acc[0], acc[1], acc[2], acc[3]);
            let (r, g, b) = if a > 0 { (r / a, g / a, b / a) } else { (0, 0, 0) };
            let o = ((dy * tw + dx) * 4) as usize;
            dst[o] = r.min(255) as u8;
            dst[o + 1] = g.min(255) as u8;
            dst[o + 2] = b.min(255) as u8;
            dst[o + 3] = (a / n).min(255) as u8;
        }
    }
    dst
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: resize-icon IN OUT SIZE [--white]");
        std::process::exit(2);
    }
    let (src, dst_path, size) = (&args[1], &args[2], args[3].parse::<u32>().expect("size"));
    let white = args.iter().any(|a| a == "--white");
    let (w, h, mut rgba) = decode(src);
    if white {
        for px in rgba.chunks_exact_mut(4) {
            if px[3] > 0 {
                px[0] = 255;
                px[1] = 255;
                px[2] = 255;
            }
        }
        encode(dst_path, w, h, &rgba);
        println!("{dst_path}: recolored {}x{}", w, h);
        return;
    }
    let (tw, th) = if size == 0 || (size == w && size == h) {
        (w, h)
    } else {
        (size, size)
    };
    let out = if (tw, th) == (w, h) { rgba.clone() } else { resize(w, h, &rgba, tw, th) };
    encode(dst_path, tw, th, &out);
    println!("{dst_path}: {}x{} -> {}x{}", w, h, tw, th);
}
