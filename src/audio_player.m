#import <AVFoundation/AVFoundation.h>

void *alloy_audio_create(const unsigned char *bytes, size_t length) {
    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:bytes length:length];
        AVAudioPlayer *player = [[AVAudioPlayer alloc] initWithData:data error:nil];
        if (!player || ![player prepareToPlay]) return NULL;
        return (__bridge_retained void *)player;
    }
}
void alloy_audio_destroy(void *handle) { @autoreleasepool { AVAudioPlayer *p = (__bridge_transfer AVAudioPlayer *)handle; [p stop]; } }
int alloy_audio_play(void *handle) { return [(__bridge AVAudioPlayer *)handle play]; }
void alloy_audio_pause(void *handle) { [(__bridge AVAudioPlayer *)handle pause]; }
void alloy_audio_rewind(void *handle) { AVAudioPlayer *p = (__bridge AVAudioPlayer *)handle; [p pause]; p.currentTime = 0; }
int alloy_audio_playing(void *handle) { return [(__bridge AVAudioPlayer *)handle isPlaying]; }
double alloy_audio_position(void *handle) { return [(__bridge AVAudioPlayer *)handle currentTime]; }
double alloy_audio_duration(void *handle) { return [(__bridge AVAudioPlayer *)handle duration]; }
