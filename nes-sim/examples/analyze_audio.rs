use nes_sim::{ControllerState, FrontendInput, FrontendRuntime};
use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() {
    let rom_path = env::args().nth(1).expect("Usage: analyze_audio <rom-path>");
    let rom = fs::read(&rom_path).expect("Failed to read ROM");
    let mut runtime = FrontendRuntime::from_rom_bytes(&rom).expect("Failed to load ROM");

    let input = FrontendInput {
        controller1: ControllerState::new(),
        ..Default::default()
    };

    // 运行几秒收集样本
    let mut all_samples = Vec::new();
    for _ in 0..300 {
        let snapshot = runtime.step(input);
        all_samples.extend_from_slice(snapshot.audio.samples);
    }

    // 分析
    let min = all_samples.iter().cloned().reduce(f32::min).unwrap();
    let max = all_samples.iter().cloned().reduce(f32::max).unwrap();
    let avg = all_samples.iter().sum::<f32>() / all_samples.len() as f32;

    // 计算方差（噪声水平）
    let variance =
        all_samples.iter().map(|&x| (x - avg).powi(2)).sum::<f32>() / all_samples.len() as f32;
    let std_dev = variance.sqrt();

    // 统计接近静音的样本
    let near_zero_count = all_samples.iter().filter(|&&x| x.abs() < 1e-6).count();
    let near_zero_ratio = near_zero_count as f32 / all_samples.len() as f32;

    println!("=== 音频样本分析 ===");
    println!("样本总数: {}", all_samples.len());
    println!("最小值: {:.6}", min);
    println!("最大值: {:.6}", max);
    println!("平均值: {:.6}", avg);
    println!("标准差 (噪声水平): {:.6}", std_dev);
    println!(
        "接近静音的样本 (<1e-6): {} ({:.2}%)",
        near_zero_count,
        near_zero_ratio * 100.0
    );

    // 找出典型静音期间的噪声范围
    let mut silent_samples: Vec<f32> = all_samples
        .iter()
        .filter(|&&x| x.abs() < 0.001)
        .copied()
        .collect();
    silent_samples.sort_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());
    if silent_samples.len() > 100 {
        let quiet_max = silent_samples[silent_samples.len() / 2 + 50]; // 取中位数附近的最大值
        println!("静音期间典型噪声范围: ±{:.6}", quiet_max);
    }

    // 保存所有样本供可视化（默认最多保存 10 秒）
    let sample_rate = runtime.snapshot().audio.sample_rate;
    let max_samples = (sample_rate as usize) * 10; // 最多 10 秒
    let wav_samples = &all_samples[..max_samples.min(all_samples.len())];
    let wav_path = Path::new(&rom_path).with_extension("wav");
    write_wav(&wav_path, wav_samples, sample_rate);
    println!(
        "已保存 {} 个样本 (约 {:.1} 秒) 到: {}",
        wav_samples.len(),
        wav_samples.len() as f32 / sample_rate as f32,
        wav_path.display()
    );
}

fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) {
    let file = fs::File::create(path).expect("Failed to create WAV file");
    let mut writer = BufWriter::new(file);

    // WAV header
    let sample_count = samples.len() as u32;
    let byte_count = sample_count * 2; // 16-bit mono

    writer.write_all(b"RIFF").unwrap();
    writer.write_all(&(36 + byte_count).to_le_bytes()).unwrap();
    writer.write_all(b"WAVE").unwrap();
    writer.write_all(b"fmt ").unwrap();
    writer.write_all(&16u32.to_le_bytes()).unwrap(); // fmt chunk size
    writer.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    writer.write_all(&1u16.to_le_bytes()).unwrap(); // mono
    writer.write_all(&sample_rate.to_le_bytes()).unwrap();
    writer.write_all(&(sample_rate * 2).to_le_bytes()).unwrap(); // byte rate
    writer.write_all(&2u16.to_le_bytes()).unwrap(); // block align
    writer.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
    writer.write_all(b"data").unwrap();
    writer.write_all(&byte_count.to_le_bytes()).unwrap();

    // samples
    for &sample in samples {
        let sample_i16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        writer.write_all(&sample_i16.to_le_bytes()).unwrap();
    }
}
