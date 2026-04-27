# Specification: my-watts GPS Analyzer

## Feature 1: GPS Drift Correction

### Problem
GPS/GNSS receivers in bike tracking applications suffer from instantaneous position drift, causing noisy coordinates that don't accurately reflect the actual route. This results in:
- Spurious distance calculations
- Inaccurate elevation profiles
- Difficult-to-read route visualizations

### Solution
Apply Savitzky-Golay smoothing to correct position drift while preserving route features (sharp turns, elevation changes).

### Algorithm Choice
**Primary**: Savitzky-Golay filter (polynomial smoothing)
**Fallback**: Kalman filter (if Savitzky-Golay proves insufficient)

The Savitzky-Golay filter works by fitting a polynomial through a sliding window of points. This approach:
- Preserves sharp features (turns, climbs) better than moving average
- Doesn't require velocity as an independent measurement
- Is well-suited for offline GPX analysis
- Works independently on latitude, longitude, and altitude

### CLI Interface

```
my-watts smooth <INPUT> [OPTIONS]

Arguments:
  INPUT                  GPX file to smooth

Options:
  -o, --output <FILE>    Output CSV file (default: input.smoothed.csv)
  --window-size <N>      Savitzky-Golay window size, must be odd (default: 5)
  --degree <N>           Polynomial degree for smoothing (default: 2)
  -v, --verbose          Enable verbose output
  -h, --help             Print help
```

### Input/Output Formats
- **Input**: GPX 1.1 format (standard bike tracking format)
- **Output**: CSV with columns: `latitude,longitude,altitude,timestamp`
- Timestamps preserved exactly (not smoothed)
- All coordinates in decimal degrees, altitude in meters

### Behavior
1. Parse GPX file, extract all trackpoints
2. Apply Savitzky-Golay smoothing to lat/lon/alt independently
3. Write smoothed coordinates to CSV
4. Report summary: number of points, window size used

### Error Handling
- Invalid GPX format → error with helpful message
- File not found → error
- Window size too large → error (must be < track length)
- Window size even or < 3 → error (must be odd and >= 3)
- Empty track → error

### Testing
- Unit tests for smoothing algorithm (verify polynomial fitting)
- Integration tests: read GPX → smooth → write CSV
- Edge cases: minimal points (1-3), large window size, exact boundaries
- Golden test: known input GPX produces expected smoothed output
