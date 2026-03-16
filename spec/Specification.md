# OpenAI Realtime Client Specification

## 1. Class Roles and Purposes

### 1.1 Client
- Main entry point for the application
- Manages the WebSocket connection to the OpenAI Realtime API
- Coordinates the flow of audio data and events between the user, the API, and other components

### 1.2 API
- Handles the low-level WebSocket communication with the OpenAI Realtime API
- Manages the connection lifecycle (connect, disconnect, reconnect)
- Sends and receives messages to/from the API

### 1.3 Conversation
- Maintains the state of the ongoing conversation
- Stores and manages audio chunks for both input and output
- Provides methods to append input audio and retrieve output audio

### 1.4 EventHandler
- Processes events received from the API
- Updates the conversation state based on received events
- Triggers appropriate actions or callbacks based on event types

### 1.5 Utils
- Provides utility functions for audio processing and data conversion
- Handles encoding and decoding of audio samples
- Offers helper methods for data manipulation and validation

## 2. Application Flow

### 2.1 Initialization
1. Client is instantiated with configuration parameters
2. API object is created and configured
3. Conversation object is initialized
4. EventHandler is set up with appropriate callbacks

### 2.2 Connection Establishment
1. Client calls API to establish WebSocket connection
2. API sends authentication message to the server
3. Server acknowledges successful connection

### 2.3 Main Application Cycle
1. User provides input audio via `appendInputAudio()`
2. Client processes and sends audio data to the API
3. API transmits audio data to the server
4. Server processes audio and generates response
5. API receives response events from the server
6. EventHandler processes received events
7. Conversation state is updated with new audio chunks
8. Client notifies user of new audio data availability

### 2.4 Application Termination
1. User or system initiates disconnection
2. Client signals API to close the connection
3. API sends disconnection message to the server
4. Server acknowledges disconnection
5. WebSocket connection is closed
6. Client releases resources and resets state

## 3. Event Types

### 3.1 Client-triggered Events
- `connect`: Initiates connection to the server
- `disconnect`: Initiates disconnection from the server
- `sendAudio`: Sends audio data to the server
- `setMute`: Toggles mute state for input audio

### 3.2 Server-triggered Events
- `message`: Received when server sends a message
- `audio`: Received when server sends audio data
- `error`: Received when server encounters an error
- `close`: Received when server closes the connection

## 4. Data Structure Handling

### 4.1 Audio Sample Encoding
- Input audio is expected to be in 16-bit PCM format
- Audio samples are stored as Int16Array in JavaScript
- When sending to the server, audio is base64 encoded

### 4.2 Audio Sample Decoding
- Received audio from the server is in base64 encoded format
- Decoded audio is converted to Int16Array for processing
- Output audio is provided as Int16Array for playback

### 4.3 Integer Types
- 16-bit integers (Int16) are used for audio sample representation
- 32-bit integers (Int32) are used for various counters and indices
- 64-bit floating-point numbers (Float64) are used for timestamps

### 4.4 Data Conversion
- Utility functions are provided for:
  - Converting between Int16Array and base64 strings
  - Resampling audio data
  - Concatenating audio chunks

## 5. Error Handling
- Errors are propagated through the system using JavaScript's native Error objects
- Each component (Client, API, Conversation) has its own set of specific error types
- Errors are caught and handled at appropriate levels, with the option to bubble up to the user

## 6. Configuration Parameters
- API endpoint URL
- Authentication tokens
- Audio settings (sample rate, channels, etc.)
- Reconnection settings (max attempts, backoff strategy)
- Event callbacks for various system events

This specification provides a comprehensive overview of the OpenAI Realtime Client's structure and behavior. It serves as a reference for implementing and testing the Rust port of the library.