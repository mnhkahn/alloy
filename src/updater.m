// Load Sparkle only from the installed app's embedded, signed framework.
// Ordinary cargo builds and UI tests need neither the SDK nor update networking.
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

@interface AlloySparkleUpdaterAPI : NSObject
@property(readonly) BOOL canCheckForUpdates;
@end
@interface AlloySparkleControllerAPI : NSObject
- (instancetype)initWithStartingUpdater:(BOOL)start updaterDelegate:(id)delegate userDriverDelegate:(id)driver;
- (void)startUpdater;
- (void)checkForUpdates:(id)sender;
@property(readonly) AlloySparkleUpdaterAPI *updater;
@end

static AlloySparkleControllerAPI *controller;
static id retainedDelegate;
static BOOL blocked = YES;
static BOOL checkingFeed = NO;
static void (^pendingInstall)(void);

@interface AlloyUpdaterDelegate : NSObject
@end
@implementation AlloyUpdaterDelegate
- (BOOL)updater:(id)updater shouldPostponeRelaunchForUpdate:(id)item untilInvokingBlock:(void (^)(void))installHandler {
    if (!blocked) return NO;
    pendingInstall = [installHandler copy];
    return YES;
}
@end

int alloy_updater_start(void) {
    @autoreleasepool {
        if (![NSThread isMainThread]) return 0;
        if (controller) return 1;
        NSBundle *app = [NSBundle mainBundle];
        if (![app objectForInfoDictionaryKey:@"SUPublicEDKey"] || ![app objectForInfoDictionaryKey:@"SUFeedURL"]) return 0;
        NSString *path = [[app bundlePath] stringByAppendingPathComponent:@"Contents/Frameworks/Sparkle.framework"];
        NSBundle *framework = [NSBundle bundleWithPath:path];
        NSError *error = nil;
        if (!framework || ![framework loadAndReturnError:&error]) {
            NSLog(@"Alloy updater could not load Sparkle: %@", error);
            return 0;
        }
        Class updaterClass = NSClassFromString(@"SPUStandardUpdaterController");
        if (!updaterClass) return 0;
        retainedDelegate = [AlloyUpdaterDelegate new];
        controller = [[updaterClass alloc] initWithStartingUpdater:NO updaterDelegate:retainedDelegate userDriverDelegate:nil];
        [controller startUpdater];
        return controller != nil;
    }
}
int alloy_updater_available(void) {
    return [NSThread isMainThread] && controller != nil;
}
int alloy_updater_can_check(void) {
    return alloy_updater_available() && !checkingFeed && controller.updater.canCheckForUpdates;
}
static NSString *feed_error_message(NSInteger status, NSError *error) {
    if (status == 404) return @"当前 GitHub Release 尚未发布更新列表。请等待包含自动更新支持的新版本发布后再检查。";
    if (error || status != 200) return @"暂时无法连接更新服务，请检查网络后重试。";
    return nil;
}
static void finish_feed_check(NSInteger status, NSError *error) {
    checkingFeed = NO;
    // A Git operation or unsaved edit may have started while the request was running.
    if (blocked || !alloy_updater_can_check()) return;
    NSString *message = feed_error_message(status, error);
    if (message) {
        NSAlert *alert = [NSAlert new];
        alert.messageText = @"暂时无法检查更新";
        alert.informativeText = message;
        [alert addButtonWithTitle:@"确定"];
        [alert runModal];
        return;
    }
    // Sparkle independently downloads and verifies the signed feed and archive.
    [controller checkForUpdates:nil];
}
int alloy_updater_check(void) {
    if (blocked || !alloy_updater_can_check()) return 0;
    NSString *feed = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"SUFeedURL"];
    NSURL *url = feed ? [NSURL URLWithString:feed] : nil;
    if (!url) return 0;
    checkingFeed = YES;
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url
        cachePolicy:NSURLRequestReloadIgnoringLocalCacheData timeoutInterval:20];
    request.HTTPMethod = @"HEAD";
    [[[NSURLSession sharedSession] dataTaskWithRequest:request completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
        NSInteger status = [response isKindOfClass:[NSHTTPURLResponse class]] ? ((NSHTTPURLResponse *)response).statusCode : 0;
        dispatch_async(dispatch_get_main_queue(), ^{ finish_feed_check(status, error); });
    }] resume];
    return 1;
}
void alloy_updater_set_blocked(int value) {
    if (![NSThread isMainThread]) return;
    blocked = value != 0;
    if (!blocked && pendingInstall) {
        void (^install)(void) = pendingInstall;
        pendingInstall = nil;
        dispatch_async(dispatch_get_main_queue(), install);
    }
}
