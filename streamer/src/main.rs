use std::{
    collections::VecDeque,
    env,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use gst::{MessageView, prelude::*};
use gstreamer as gst;
use gstreamer_app::{AppSink, AppSinkCallbacks, AppSrc};
use tokio::net::UnixStream;
use tokio::{
    process::{Child, Command},
    time,
};

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
fn escaped(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn sanitize_stream_error(message: &str, output: &str) -> String {
    if !(output.starts_with("rtmp://") || output.starts_with("rtmps://")) {
        return message.to_owned();
    }
    let mut safe = message.replace(output, "RTMP destination");
    if let Some(key) = output.rsplit('/').next().filter(|part| !part.is_empty()) {
        safe = safe.replace(key, "[redacted]");
    }
    safe
}

fn redacted_error(error: &anyhow::Error, output: &str) -> anyhow::Error {
    anyhow!(sanitize_stream_error(&format!("{error:#}"), output))
}

#[derive(Debug)]
struct MusicPlaylist {
    tracks: Vec<String>,
    queue: VecDeque<String>,
    remaining: usize,
    random_state: u64,
}

struct MusicPlayback {
    pipeline: gst::Pipeline,
    active: Arc<AtomicBool>,
}

impl MusicPlaylist {
    fn new(tracks: Vec<String>, entries: usize) -> Result<Self> {
        if tracks.is_empty() {
            anyhow::bail!("empty music playlist");
        }
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Ok(Self {
            tracks,
            queue: VecDeque::new(),
            remaining: entries,
            random_state: seed.max(1),
        })
    }

    fn next(&mut self) -> Option<String> {
        if self.remaining == 0 {
            return None;
        }
        if self.queue.is_empty() {
            self.queue = self.tracks.iter().cloned().collect();
            for i in (1..self.queue.len()).rev() {
                self.random_state ^= self.random_state << 13;
                self.random_state ^= self.random_state >> 7;
                self.random_state ^= self.random_state << 17;
                self.queue.swap(i, self.random_state as usize % (i + 1));
            }
        }
        self.remaining -= 1;
        self.queue.pop_front()
    }
}

fn short_fade_gain(media_time: f64) -> f64 {
    let fade_in = (media_time / 0.5).clamp(0.0, 1.0);
    let fade_out = if media_time > 29.5 {
        ((31.0 - media_time) / 1.5).clamp(0.0, 1.0)
    } else {
        1.0
    };
    fade_in * fade_out
}

fn attach_short_fades(volume: &gst::Element) -> Result<()> {
    let pad = volume
        .static_pad("sink")
        .context("music volume sink pad missing")?;
    let volume = volume.clone();
    let media_origin = Arc::new(Mutex::new(None::<u64>));
    let media_origin_probe = media_origin.clone();
    pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
        if let Some(time) = info.buffer().and_then(|buffer| buffer.pts()) {
            let mut origin = media_origin_probe.lock().expect("fade time origin mutex");
            let first_pts = *origin.get_or_insert(time.nseconds());
            let media_time = time.nseconds().saturating_sub(first_pts) as f64 / 1_000_000_000.0;
            volume.set_property("volume", short_fade_gain(media_time));
        }
        gst::PadProbeReturn::Ok
    });
    Ok(())
}

fn pipeline(output: &str, short: bool, seconds: Option<u64>) -> String {
    let width = value("WIDTH", if short { "1080" } else { "2560" });
    let height = value("HEIGHT", if short { "1920" } else { "1440" });
    let capture = value("CAPTURE_FPS", "30");
    let fps = value("FPS", "30");
    let kbps = value("VIDEO_BITRATE", "12000k")
        .trim_end_matches('k')
        .parse::<u32>()
        .unwrap_or(12000);
    let buffer_kbits = value("BUFSIZE", "24000k")
        .trim_end_matches('k')
        .parse::<u32>()
        .unwrap_or(24000);
    let buffer_ms = (buffer_kbits.saturating_mul(1000) / kbps.max(1)).clamp(100, 10000);
    let audio_bps = value("AUDIO_BITRATE", "128k")
        .trim_end_matches('k')
        .parse::<u32>()
        .unwrap_or(128)
        * 1000;
    let preset = value("PRESET", if short { "veryfast" } else { "superfast" });
    let gop = fps.parse::<u32>().unwrap_or(30) * 2;
    let encoder_threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    let frame_rate_conversion = if capture.parse::<u32>().ok() != fps.parse::<u32>().ok() {
        "videorate ! "
    } else {
        ""
    };
    let frame_limit = seconds.map_or_else(String::new, |seconds| {
        format!(
            " num-buffers={}",
            capture.parse::<u64>().unwrap_or(30).saturating_mul(seconds)
        )
    });
    let loudness_target = if short { -27 } else { -22 };
    let x264_options =
        format!("min-keyint={gop}:scenecut=0:colorprim=bt709:transfer=bt709:colormatrix=bt709");
    let rate_control = if short {
        format!("pass=qual quantizer=20 option-string={x264_options}")
    } else {
        format!(
            "pass=cbr bitrate={kbps} vbv-buf-capacity={buffer_ms} option-string=nal-hrd=cbr:{x264_options}"
        )
    };
    let display = if short { ":98.0" } else { ":99.0" };
    let video = format!(
        "ximagesrc name=video_source display-name={display} use-damage=false show-pointer=false{frame_limit} ! video/x-raw,framerate={capture}/1 ! \
         {frame_rate_conversion}videoconvert n-threads={encoder_threads} ! video/x-raw,width={width},height={height},format=I420,framerate={fps}/1,colorimetry=bt709 ! \
         x264enc name=video_encoder speed-preset={preset} threads={encoder_threads} {rate_control} key-int-max={gop} bframes=3 rc-lookahead=0 ! h264parse name=video_parser config-interval=-1 ! queue name=video_queue max-size-time=10000000000 max-size-bytes=0 max-size-buffers=0 ! mux."
    );
    let audio = if music_tracks().is_empty() {
        let silence_limit = seconds.map_or_else(String::new, |seconds| {
            format!(" num-buffers={}", seconds.saturating_mul(44_100) / 1024)
        });
        format!(
            "audiotestsrc name=silence_source is-live=true wave=silence{silence_limit} ! audio/x-raw,format=S16LE,rate=44100,channels=2 ! voaacenc bitrate={audio_bps} ! aacparse ! queue ! mux."
        )
    } else {
        format!(
            "appsrc name=music_src is-live=true format=time do-timestamp=true ! audioconvert ! audioresample ! audioloudnorm name=loudness loudness-target={loudness_target} loudness-range-target=11 max-true-peak=-2 ! audioconvert ! audioresample ! volume name=music_volume ! audioconvert ! audio/x-raw,format=S16LE,rate=44100,channels=2 ! voaacenc bitrate={audio_bps} ! aacparse ! queue name=audio_queue ! mux."
        )
    };
    let sink = if output.starts_with("rtmp://") || output.starts_with("rtmps://") {
        format!(
            "flvmux name=mux streamable=true ! rtmpsink location=\"{}\"",
            escaped(output)
        )
    } else {
        format!(
            "mp4mux name=mux faststart=true ! filesink location=\"{}\"",
            escaped(output)
        )
    };
    format!("{video} {audio} {sink}")
}

fn music_tracks() -> Vec<PathBuf> {
    if let Ok(track) = env::var("TRACK") {
        if !track.is_empty() {
            return vec![PathBuf::from(track)];
        }
    }
    let Ok(entries) = std::fs::read_dir("/music") else {
        return Vec::new();
    };
    let mut tracks: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                ["mp3", "m4a", "ogg", "flac", "wav"].contains(&x.to_ascii_lowercase().as_str())
            })
        })
        .collect();
    tracks.sort();
    if !tracks.is_empty() {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as usize;
        let rotate = seed % tracks.len();
        tracks.rotate_left(rotate);
        // Fisher–Yates with a changing xorshift state avoids a runtime dependency for shuffling.
        let mut state = seed.max(1);
        for i in (1..tracks.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            tracks.swap(i, state % (i + 1));
        }
    }
    tracks
}

fn attach_music(
    capture_pipeline: &gst::Pipeline,
    tracks: &[PathBuf],
    loop_music: bool,
) -> Result<Option<MusicPlayback>> {
    if tracks.is_empty() {
        return Ok(None);
    }
    let playbin = gst::ElementFactory::make("playbin")
        .name("music-player")
        .build()?;
    let sink_element = gst::ElementFactory::make("appsink").build()?;
    let audio_sink = sink_element
        .downcast::<AppSink>()
        .map_err(|_| anyhow!("appsink factory returned the wrong type"))?;
    audio_sink.set_property("sync", true);
    let src_element = capture_pipeline
        .by_name("music_src")
        .context("music appsrc missing")?;
    let audio_src = src_element
        .downcast::<AppSrc>()
        .map_err(|_| anyhow!("appsrc factory returned the wrong type"))?;
    let bridge = audio_src.clone();
    let music_active = Arc::new(AtomicBool::new(true));
    let active = music_active.clone();
    audio_sink.set_callbacks(
        AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                if active.load(Ordering::Relaxed) {
                    let mut buffer = sample.buffer().ok_or(gst::FlowError::Error)?.copy();
                    if let Some(buffer) = buffer.get_mut() {
                        // The decoded file uses its own timeline. Re-timestamp on insertion so
                        // music shares the live capture pipeline's clock and muxer timeline.
                        buffer.set_pts(None);
                        buffer.set_dts(None);
                    }
                    let caps = sample.caps().map(|caps| caps.to_owned());
                    bridge.set_caps(caps.as_ref());
                    bridge.push_buffer(buffer)
                } else {
                    Ok(gst::FlowSuccess::Ok)
                }
            })
            .build(),
    );
    playbin.set_property("audio-sink", &audio_sink);
    let fake_video = gst::ElementFactory::make("fakesink").build()?;
    playbin.set_property("video-sink", &fake_video);
    let music_pipeline = gst::Pipeline::new();
    music_pipeline.add(&playbin)?;
    let playlist_tracks: Vec<String> = tracks
        .iter()
        .filter_map(|p| url::Url::from_file_path(p).ok().map(|u| u.to_string()))
        .collect();
    let entries = if loop_music {
        20_000
    } else {
        playlist_tracks.len()
    };
    let mut playlist = MusicPlaylist::new(playlist_tracks, entries)?;
    let initial = playlist.next().context("empty music playlist")?;
    playbin.set_property("uri", &initial);
    if loop_music {
        let shared = Arc::new(Mutex::new(playlist));
        playbin.connect("about-to-finish", false, move |args| {
            let element = args[0]
                .get::<gst::Element>()
                .expect("playbin signal instance");
            if let Some(next) = shared.lock().expect("music playlist mutex").next() {
                element.set_property("uri", &next);
            }
            None
        });
    }
    Ok(Some(MusicPlayback {
        pipeline: music_pipeline,
        active: music_active,
    }))
}

fn run_pipeline(output: &str, short: bool, seconds: Option<u64>) -> Result<()> {
    let desc = pipeline(output, short, seconds);
    let element = gst::parse::launch(&desc).context("could not build GStreamer pipeline")?;
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("GStreamer did not return a pipeline"))?;
    let tracks = music_tracks();
    let music = attach_music(&pipeline, &tracks, !short && seconds.is_none())?;
    let bus = pipeline.bus().context("pipeline has no bus")?;
    let music_volume = pipeline.by_name("music_volume");
    if short {
        if let Some(volume) = &music_volume {
            volume.set_property("volume", 0.0f64);
            attach_short_fades(volume)?;
        }
    }
    pipeline.set_state(gst::State::Playing)?;
    if let Some(music) = &music {
        music.pipeline.set_state(gst::State::Playing)?;
    }
    if let Some(seconds) = seconds {
        eprintln!("[streamer] finite recording started ({seconds}s)");
    }
    let started = std::time::Instant::now();
    let deadline = seconds.map(|s| started + Duration::from_secs(s));
    let mut pipeline_finished = false;
    loop {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            if seconds.is_some() {
                eprintln!("[streamer] finite recording duration reached; draining muxer");
            }
            break;
        }
        if let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
            match message.view() {
                MessageView::Eos(..) => {
                    pipeline_finished = true;
                    if seconds.is_some() {
                        eprintln!("[streamer] finite recording muxer reached EOS");
                    }
                    break;
                }
                MessageView::Error(e) => {
                    pipeline.set_state(gst::State::Null).ok();
                    let error = anyhow!(
                        "GStreamer {}: {} ({:?})",
                        e.src().map(|s| s.path_string()).unwrap_or_default(),
                        e.error(),
                        e.debug()
                    );
                    return Err(redacted_error(&error, output));
                }
                _ => {}
            }
        }
    }
    // Stop music playback at the recording deadline, end appsrc, and let the
    // muxer drain the buffered audio/video before finalizing the output.
    if seconds.is_some() && !pipeline_finished {
        if let Some(music) = &music {
            eprintln!("[streamer] stopping finite music source");
            music.active.store(false, Ordering::Relaxed);
            music.pipeline.set_state(gst::State::Null)?;
            eprintln!("[streamer] finite music source stopped");
        }
        if let Some(src) = pipeline
            .by_name("music_src")
            .and_then(|src| src.downcast::<AppSrc>().ok())
        {
            eprintln!("[streamer] ending finite music appsrc");
            src.end_of_stream()?;
        }
    }
    let eos_deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !pipeline_finished {
        if let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(250)) {
            match message.view() {
                MessageView::Eos(..) => {
                    if seconds.is_some() {
                        eprintln!("[streamer] finite recording muxer reached EOS");
                    }
                    break;
                }
                MessageView::Error(e) => {
                    pipeline.set_state(gst::State::Null).ok();
                    let error = anyhow!(
                        "GStreamer {}: {} ({:?})",
                        e.src().map(|s| s.path_string()).unwrap_or_default(),
                        e.error(),
                        e.debug()
                    );
                    return Err(redacted_error(&error, output));
                }
                _ => {}
            }
        }
        if std::time::Instant::now() >= eos_deadline {
            pipeline.set_state(gst::State::Null).ok();
            anyhow::bail!("GStreamer did not finish muxing before the 30 second deadline");
        }
    }
    pipeline.set_state(gst::State::Null)?;
    if let Some(music) = &music {
        music.pipeline.set_state(gst::State::Null)?;
    }
    Ok(())
}

async fn wait_dashboard() -> Result<()> {
    let url = format!(
        "{}/healthz",
        value("DASHBOARD_URL", "http://dashboard:8080/").trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    loop {
        if client
            .get(&url)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(());
        }
        time::sleep(Duration::from_secs(3)).await;
    }
}

async fn upload_short(path: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()?;
    let client_id = env::var("YOUTUBE_CLIENT_ID").context("YOUTUBE_CLIENT_ID is required")?;
    let client_secret =
        env::var("YOUTUBE_CLIENT_SECRET").context("YOUTUBE_CLIENT_SECRET is required")?;
    let refresh = env::var("YOUTUBE_REFRESH_TOKEN").context("YOUTUBE_REFRESH_TOKEN is required")?;
    let token_url = value("GOOGLE_TOKEN_URL", "https://oauth2.googleapis.com/token");
    let token: serde_json::Value = client
        .post(token_url)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let access = token["access_token"]
        .as_str()
        .context("Google token response omitted access_token")?;
    let metadata = tokio::fs::read(format!("{path}.json"))
        .await
        .context("Short metadata JSON is missing")?;
    let bytes = tokio::fs::read(path).await?;
    let upload_url = value(
        "YOUTUBE_UPLOAD_URL",
        "https://www.googleapis.com/upload/youtube/v3/videos",
    );
    let response = client
        .post(format!(
            "{upload_url}?uploadType=resumable&part=snippet,status"
        ))
        .bearer_auth(access)
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", "video/mp4")
        .header("X-Upload-Content-Length", bytes.len())
        .body(metadata)
        .send()
        .await?
        .error_for_status()?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .context("YouTube did not return a resumable upload URL")?
        .to_str()?
        .to_owned();
    let uploaded: serde_json::Value = client
        .put(location)
        .bearer_auth(access)
        .header("Content-Type", "video/mp4")
        .body(bytes)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!(
        "Uploaded https://www.youtube.com/shorts/{} ({})",
        uploaded["id"].as_str().unwrap_or("unknown"),
        uploaded["status"]["privacyStatus"]
            .as_str()
            .unwrap_or("unknown privacy")
    );
    Ok(())
}

async fn browser_loop(
    url: String,
    width: String,
    height: String,
    display: String,
    restart: u64,
) -> Result<()> {
    let scale = env::var("SCALE")
        .unwrap_or_else(|_| format!("{:.6}", width.parse::<f64>().unwrap_or(2560.0) / 1920.0));
    loop {
        let user_data = if display == ":98.0" {
            "/tmp/chromium-short"
        } else {
            "/tmp/chromium"
        };
        let _ = std::fs::remove_dir_all(user_data);
        let mut browser = Command::new("chromium")
            .args([
                "--no-sandbox",
                "--no-first-run",
                "--noerrdialogs",
                "--disable-infobars",
                "--kiosk",
                "--window-position=0,0",
                &format!("--window-size={width},{height}"),
                &format!("--force-device-scale-factor={scale}"),
                "--hide-scrollbars",
                "--disable-translate",
                "--disable-features=Translate,MediaRouter",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
                "--disable-backgrounding-occluded-windows",
                "--disable-dev-shm-usage",
                if display == ":98.0" {
                    "--user-data-dir=/tmp/chromium-short"
                } else {
                    "--user-data-dir=/tmp/chromium"
                },
                &format!("--app={url}"),
            ])
            .env("DISPLAY", &display)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        tokio::select! {
            _=time::sleep(Duration::from_secs(restart*3600))=>{let _=browser.kill().await;},
            _=browser.wait()=>{}
        }
        time::sleep(Duration::from_secs(3)).await;
    }
}

async fn start_xvfb(display: &str, width: &str, height: &str, log: &str) -> Result<Child> {
    let display_number = display
        .strip_prefix(':')
        .unwrap_or(display)
        .split('.')
        .next()
        .context("invalid X display name")?;
    let lock = format!("/tmp/.X{display_number}-lock");
    let socket = format!("/tmp/.X11-unix/X{display_number}");
    let _ = std::fs::remove_file(lock);
    let _ = std::fs::remove_file(socket);
    let file = std::fs::File::create(log)?;
    let mut child = Command::new("Xvfb")
        .args([
            display,
            "-screen",
            "0",
            &format!("{width}x{height}x24"),
            "-nolisten",
            "tcp",
            "-ac",
        ])
        .stdout(file.try_clone()?)
        .stderr(file)
        .kill_on_drop(true)
        .spawn()?;
    let socket = format!("/tmp/.X11-unix/X{display_number}");
    for _ in 0..50 {
        if child.try_wait()?.is_some() {
            anyhow::bail!("Xvfb exited before display {display} became ready");
        }
        if UnixStream::connect(&socket).await.is_ok() {
            return Ok(child);
        }
        time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    anyhow::bail!("Xvfb display {display} did not become ready within 5 seconds");
}

fn lower_short_encode_priority() {
    #[cfg(unix)]
    {
        // The Short runs alongside the live stream and must yield CPU to it.
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 10) } != 0 {
            eprintln!(
                "[streamer] could not lower Short encoder priority: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    gst::init()?;
    if env::args().nth(1).as_deref() == Some("upload-short") {
        let path = env::args()
            .nth(2)
            .context("usage: anyps5-live-streamer upload-short <file.mp4>")?;
        upload_short(&path).await?;
        return Ok(());
    }
    if env::args().nth(1).as_deref() == Some("short-meta") {
        let url = format!(
            "{}/api/short-meta",
            value("DASHBOARD_URL", "http://dashboard:8080/").trim_end_matches('/')
        );
        let body = reqwest::get(url).await?.error_for_status()?.bytes().await?;
        use std::io::Write;
        std::io::stdout().write_all(&body)?;
        return Ok(());
    }
    if env::args().nth(1).as_deref() == Some("encode") {
        let output = env::var("OUTPUT").context("OUTPUT must be set")?;
        let seconds = env::var("RECORD_SECONDS").ok().and_then(|v| v.parse().ok());
        lower_short_encode_priority();
        run_pipeline(&output, true, seconds).map_err(|error| redacted_error(&error, &output))?;
        return Ok(());
    }
    if env::args().nth(1).as_deref() == Some("record") {
        let output = env::var("OUTPUT").context("OUTPUT must be set to an MP4 path")?;
        let seconds = env::var("RECORD_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let mut xvfb = start_xvfb(
            ":98",
            &value("WIDTH", "1080"),
            &value("HEIGHT", "1920"),
            "/tmp/xvfb-short.log",
        )
        .await?;
        let _ = std::fs::remove_dir_all("/tmp/chromium-short");
        let start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() + 10_000;
        let page = format!(
            "{}/short.html?start={start}",
            value("DASHBOARD_URL", "http://dashboard:8080/").trim_end_matches('/')
        );
        let _browser = tokio::spawn(browser_loop(
            page,
            "1080".into(),
            "1920".into(),
            ":98".into(),
            24,
        ));
        tokio::time::sleep(Duration::from_secs(10)).await;
        run_pipeline(&output, true, Some(seconds))
            .map_err(|error| redacted_error(&error, &output))?;
        let _ = xvfb.kill().await;
        let _ = std::fs::remove_dir_all("/tmp/chromium-short");
        return Ok(());
    }

    let output = env::var("OUTPUT")
        .ok()
        .filter(|x| !x.is_empty())
        .or_else(|| {
            env::var("YOUTUBE_STREAM_KEY")
                .ok()
                .filter(|x| !x.is_empty())
                .map(|key| {
                    format!(
                        "{}/{}",
                        value("YOUTUBE_RTMP_URL", "rtmp://a.rtmp.youtube.com/live2")
                            .trim_end_matches('/'),
                        key
                    )
                })
        })
        .context("set YOUTUBE_STREAM_KEY or OUTPUT")?;
    wait_dashboard().await?;
    let mut xvfb = start_xvfb(
        ":99",
        &value("WIDTH", "2560"),
        &value("HEIGHT", "1440"),
        "/tmp/xvfb.log",
    )
    .await?;
    let dashboard_url = value("DASHBOARD_URL", "http://dashboard:8080/")
        .trim_end_matches('/')
        .to_owned();
    let page = value("PAGE", "");
    let page_url = format!("{dashboard_url}/{}", page.trim_start_matches('/'));
    let _browser = tokio::spawn(browser_loop(
        page_url,
        value("WIDTH", "2560"),
        value("HEIGHT", "1440"),
        ":99".into(),
        value("BROWSER_RESTART_HOURS", "24")
            .parse::<u64>()
            .unwrap_or(24),
    ));
    time::sleep(Duration::from_secs(
        value("WARMUP_SECONDS", "15").parse::<u64>().unwrap_or(15),
    ))
    .await;
    let record = env::var("RECORD_SECONDS")
        .ok()
        .and_then(|x| x.parse::<u64>().ok());
    loop {
        let target = if output.starts_with("rtmp://") || output.starts_with("rtmps://") {
            "RTMP destination".into()
        } else {
            output.clone()
        };
        println!("[streamer] GStreamer live pipeline → {target}");
        let result = tokio::task::spawn_blocking({
            let output = output.clone();
            move || run_pipeline(&output, false, record)
        })
        .await?;
        if record.is_some() {
            result.map_err(|error| redacted_error(&error, &output))?;
            break;
        }
        if let Err(error) = result {
            eprintln!(
                "[streamer] pipeline stopped: {}; reconnecting in 5s",
                sanitize_stream_error(&format!("{error:#}"), &output)
            );
        }
        if xvfb.try_wait()?.is_some() {
            return Err(anyhow!("Xvfb stopped"));
        }
        time::sleep(Duration::from_secs(5)).await;
    }
    let _ = xvfb.kill().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtmp_errors_never_include_the_stream_key() {
        let target = "rtmps://a.rtmp.youtube.com/live2/secret-stream-key";
        let error = format!("Could not connect to RTMP stream {target}");
        let safe = sanitize_stream_error(&error, target);
        assert!(safe.contains("RTMP destination"));
        assert!(!safe.contains("secret-stream-key"));
    }

    #[test]
    fn playlist_is_bounded_and_reshuffles_each_round() {
        let mut playlist = MusicPlaylist::new(vec!["a".into(), "b".into(), "c".into()], 8).unwrap();
        let entries: Vec<_> = std::iter::from_fn(|| playlist.next()).collect();
        assert_eq!(entries.len(), 8);
        assert_eq!(playlist.remaining, 0);
        for round in [0..3, 3..6] {
            let mut tracks = entries[round].to_vec();
            tracks.sort();
            assert_eq!(tracks, ["a", "b", "c"]);
        }
    }

    #[test]
    fn short_fades_follow_the_audio_timeline() {
        assert_eq!(short_fade_gain(0.0), 0.0);
        assert_eq!(short_fade_gain(0.5), 1.0);
        assert_eq!(short_fade_gain(29.5), 1.0);
        assert_eq!(short_fade_gain(31.0), 0.0);
        assert!(short_fade_gain(30.25) > 0.0 && short_fade_gain(30.25) < 1.0);
    }
}
