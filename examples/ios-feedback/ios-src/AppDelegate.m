//
//  AppDelegate.m
//  cpal-ios-example
//
//  Created by Michael Hills on 2/10/20.
//

#import "AppDelegate.h"
@import AVFoundation;

void rust_ios_main(void);


@interface AppDelegate ()

@end

@implementation AppDelegate



- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    // Override point for customization after application launch.

    NSError *error;
    BOOL success;

    // It is necessary to access the sharedInstance so that calls to AudioSessionGetProperty
    // will work.
    AVAudioSession *session = AVAudioSession.sharedInstance;
    // Setting up the category is not necessary, but generally advised.
    // Since this demo records and plays, lets use AVAudioSessionCategoryPlayAndRecord.
    // Also default to speaker as defaulting to the phone earpiece would be unusual.
    // Allowing bluetooth should direct audio to your bluetooth headset.
    success = [session setCategory:AVAudioSessionCategoryPlayAndRecord
                       withOptions:AVAudioSessionCategoryOptionDefaultToSpeaker | AVAudioSessionCategoryOptionAllowBluetooth
                             error:&error];

    if (success) {
        NSLog(@"Calling rust_ios_main()");
        rust_ios_main();
    } else {
        NSLog(@"Failed to configure audio session category");
    }

    return YES;
}

@end
