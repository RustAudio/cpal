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

    // It is necessary to access the sharedInstance so that calls to AudioSessionGetProperty
    // will work.
    AVAudioSession *session = AVAudioSession.sharedInstance;
    // Setting up the category is not necessary, but generally advised.
    // Since this demo records and plays, lets use AVAudioSessionCategoryPlayAndRecord.
    // Also default to speaker as defaulting to the phone earpiece would be unusual.
    // Allowing bluetooth should direct audio to your bluetooth headset.
    NSError *categoryError = nil;
    BOOL isSetCategorySuccess = [session setCategory:AVAudioSessionCategoryPlayAndRecord
                                         withOptions:AVAudioSessionCategoryOptionDefaultToSpeaker | AVAudioSessionCategoryOptionAllowBluetooth
                                               error:&categoryError];
    if (isSetCategorySuccess) {
        NSError *activateError = nil;
        BOOL isActivateSuccess = [session setActive:YES error:&activateError];

        if (isActivateSuccess) {
            NSLog(@"Calling rust_ios_main()");
            rust_ios_main();
        } else {
            NSLog(@"Failed to activate audio session: %@", activateError);
        }
    } else {
        NSLog(@"Failed to set category: %@", categoryError);
    }

    return YES;
}

@end
