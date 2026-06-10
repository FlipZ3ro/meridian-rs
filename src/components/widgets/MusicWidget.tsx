'use client';

import { useEffect, useRef, useState } from 'react';
import { Music, Pause, Play, Repeat, Shuffle, SkipBack, SkipForward, Volume2 } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';

type YouTubePlayer = {
  cueVideoById: (videoId: string) => void;
  loadVideoById: (videoId: string, startSeconds?: number) => void;
  playVideo: () => void;
  pauseVideo: () => void;
  getCurrentTime: () => number;
  setVolume: (volume: number) => void;
  unMute: () => void;
};

declare global {
  interface Window {
    YT?: {
      Player: new (
        elementId: string,
        options: {
          videoId: string;
          playerVars?: Record<string, number>;
          events?: {
            onReady?: (event: { target: YouTubePlayer }) => void;
            onStateChange?: (event: { data: number }) => void;
          };
        },
      ) => YouTubePlayer;
      PlayerState?: { PLAYING: number; PAUSED: number; ENDED: number };
    };
    onYouTubeIframeAPIReady?: () => void;
  }
}

const playlist = [
  {
    title: 'HEAL (feat. Venes)',
    artist: 'Weird Genius & Winky Wiryawan',
    videoId: 'h_iJBFEzrhI',
    duration: '4:11',
    durationSeconds: 251,
  },
  {
    title: 'Your Side (feat. Novia Bachmid)',
    artist: 'Weird Genius',
    videoId: 'BXlW3QH_5XY',
    duration: '3:20',
    durationSeconds: 200,
  },
  {
    title: 'Sweet Scar (ft. Prince Husein)',
    artist: 'Weird Genius',
    videoId: 'dxIG9JtakBM',
    duration: '3:39',
    durationSeconds: 219,
  },
];

const formatTime = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}:${String(remainingSeconds).padStart(2, '0')}`;
};

const hasPlayerMethod = <T extends keyof YouTubePlayer>(player: YouTubePlayer | null, method: T) =>
  typeof player?.[method] === 'function';

export const MusicWidget = () => {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [volume, setVolume] = useState(72);
  const playerRef = useRef<YouTubePlayer | null>(null);
  const currentSong = playlist[currentIndex];
  const thumbnailUrl = `https://i.ytimg.com/vi/${currentSong.videoId}/hqdefault.jpg`;
  const progress = `${Math.min((currentTime / currentSong.durationSeconds) * 100, 100)}%`;
  const volumeBars = Array.from({ length: 12 }, (_, index) => index < Math.round(volume / 100 * 12));

  useEffect(() => {
    const createPlayer = () => {
      if (!window.YT || playerRef.current) return;

      playerRef.current = new window.YT.Player('youtube-audio-player', {
        videoId: playlist[0].videoId,
        playerVars: { controls: 0, disablekb: 1, modestbranding: 1, rel: 0 },
        events: {
          onReady: (event) => {
            event.target.setVolume(volume);
            event.target.unMute();
            event.target.cueVideoById(playlist[0].videoId);
          },
          onStateChange: (event) => {
            if (!window.YT?.PlayerState) return;
            setIsPlaying(event.data === window.YT.PlayerState.PLAYING);
            if (event.data === window.YT.PlayerState.ENDED) playNext();
          },
        },
      });
    };

    if (window.YT?.Player) {
      createPlayer();
      return;
    }

    window.onYouTubeIframeAPIReady = createPlayer;
    const existingScript = document.querySelector('script[src="https://www.youtube.com/iframe_api"]');
    if (!existingScript) {
      const script = document.createElement('script');
      script.src = 'https://www.youtube.com/iframe_api';
      document.body.appendChild(script);
    }
  }, []);

  useEffect(() => {
    if (hasPlayerMethod(playerRef.current, 'setVolume')) playerRef.current?.setVolume(volume);
  }, [volume]);

  useEffect(() => {
    if (!isPlaying || !playerRef.current) return;

    const timer = window.setInterval(() => {
      if (hasPlayerMethod(playerRef.current, 'getCurrentTime')) {
        setCurrentTime(Math.floor(playerRef.current?.getCurrentTime() ?? 0));
      }
    }, 500);

    return () => window.clearInterval(timer);
  }, [isPlaying]);

  const selectSong = (index: number) => {
    setCurrentIndex(index);
    setCurrentTime(0);
    setIsPlaying(true);
    if (hasPlayerMethod(playerRef.current, 'unMute')) playerRef.current?.unMute();
    if (hasPlayerMethod(playerRef.current, 'setVolume')) playerRef.current?.setVolume(volume);
    if (hasPlayerMethod(playerRef.current, 'loadVideoById')) playerRef.current?.loadVideoById(playlist[index].videoId, 0);
  };

  const playPrevious = () => selectSong(currentIndex === 0 ? playlist.length - 1 : currentIndex - 1);
  const playNext = () => selectSong((currentIndex + 1) % playlist.length);

  const togglePlayback = () => {
    if (!playerRef.current) return;
    if (isPlaying) {
      if (hasPlayerMethod(playerRef.current, 'pauseVideo')) playerRef.current.pauseVideo();
      setIsPlaying(false);
      return;
    }

    if (hasPlayerMethod(playerRef.current, 'unMute')) playerRef.current.unMute();
    if (hasPlayerMethod(playerRef.current, 'setVolume')) playerRef.current.setVolume(volume);
    if (currentTime === 0 && hasPlayerMethod(playerRef.current, 'loadVideoById')) playerRef.current.loadVideoById(currentSong.videoId, 0);
    else if (hasPlayerMethod(playerRef.current, 'playVideo')) playerRef.current.playVideo();
    setIsPlaying(true);
  };

  return (
    <GlassCard className="music-card terminal-player">
      <div className="terminal-title"><Music size={18} />NOW PLAYING</div>
      <div className="terminal-divider" />
      <div className="terminal-now">
        <button type="button" className="terminal-cover video-art" onClick={togglePlayback} aria-label={isPlaying ? 'Pause song' : 'Play song'}>
          <div id="youtube-audio-player" className="youtube-audio-player" />
          <img src={thumbnailUrl} alt={`${currentSong.title} cover`} />
          <span className="cover-play"><Play size={22} fill="currentColor" /></span>
        </button>
        <div className="terminal-meta">
          <div className="meta-grid">
            <span>TITLE</span><b>:</b><strong>{currentSong.title}</strong>
            <span>ARTIST</span><b>:</b><em>{currentSong.artist}</em>
            <span>ALBUM</span><b>:</b><em>{currentSong.title}</em>
            <span>YEAR</span><b>:</b><em>2023</em>
            <span>GENRE</span><b>:</b><em>Electronic</em>
            <span>BITRATE</span><b>:</b><em>320 kbps</em>
          </div>
        </div>
        <label className="terminal-volume">
          <Volume2 size={16} />
          <span>VOL</span>
          <input type="range" min="0" max="100" value={volume} onChange={(event) => setVolume(Number(event.target.value))} />
          <span className="volume-ascii">[{volumeBars.map((active, index) => <i className={active ? 'on' : ''} key={index}>|</i>)}]</span>
          <strong>{volume}%</strong>
        </label>
      </div>
      <div className="terminal-progress">
        <span>{formatTime(currentTime)}</span>
        <div className="progress"><span style={{ width: progress }} /></div>
        <span>{currentSong.duration}</span>
      </div>
      <div className="terminal-controls">
        <button type="button" aria-label="Shuffle"><span>[SHUFFLE]</span><Shuffle size={18} /></button>
        <button type="button" onClick={playPrevious} aria-label="Previous song"><SkipBack size={22} fill="currentColor" /></button>
        <button type="button" className="primary-play" onClick={togglePlayback} aria-label={isPlaying ? 'Pause song' : 'Play song'}>
          {isPlaying ? <Pause size={23} fill="currentColor" /> : <Play size={23} fill="currentColor" />}
        </button>
        <button type="button" onClick={playNext} aria-label="Next song"><SkipForward size={22} fill="currentColor" /></button>
        <button type="button" aria-label="Repeat"><Repeat size={18} /><span>[REPEAT]</span></button>
      </div>
    </GlassCard>
  );
};
