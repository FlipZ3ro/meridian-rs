'use client';

import { useEffect, useState } from 'react';
import { Cloud } from 'lucide-react';
import { GlassCard } from '../ui/GlassCard';

type Weather = {
  temperature: number;
  condition: string;
  location: string;
  humidity: number;
  wind: string;
  pressure: string;
  forecast: Array<{ day: string; temp: number }>;
  source: string;
};

const fallbackWeather: Weather = {
  temperature: 21,
  condition: 'Cloudy',
  location: 'Magelang, Jawa Tengah',
  humidity: 86,
  wind: '2.9 km/h',
  pressure: '972 hPa',
  forecast: [
    { day: 'Sat', temp: 28 },
    { day: 'Sun', temp: 28 },
    { day: 'Mon', temp: 28 },
    { day: 'Tue', temp: 28 },
    { day: 'Wed', temp: 28 },
  ],
  source: '#',
};

export const WeatherWidget = () => {
  const [weather, setWeather] = useState(fallbackWeather);

  useEffect(() => {
    let isMounted = true;

    fetch('/api/weather')
      .then((response) => response.json())
      .then((data: Weather) => {
        if (isMounted) setWeather(data);
      })
      .catch(() => {
        if (isMounted) setWeather(fallbackWeather);
      });

    return () => {
      isMounted = false;
    };
  }, []);

  return (
    <GlassCard className="weather-card">
      <div className="terminal-title"><Cloud size={18} fill="currentColor" />WEATHER</div>
      <div className="terminal-divider" />
      <div className="weather-main">
        <Cloud size={36} fill="currentColor" />
        <div className="weather-temp"><strong>{weather.temperature}°C</strong><span>{weather.location}</span><span>{weather.condition}</span></div>
        <div className="weather-meta"><span>HUMID <b>{weather.humidity}%</b></span><span>WIND <b>{weather.wind}</b></span><span>PRESS <b>{weather.pressure}</b></span></div>
      </div>
      <div className="forecast">
        {weather.forecast.map((item) => <div key={item.day}><span>{item.day}</span><Cloud size={20} fill="currentColor" /><b>{item.temp}°</b></div>)}
      </div>
    </GlassCard>
  );
};
