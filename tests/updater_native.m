// Isolated main-thread updater tests: no framework loading or network access.
#import "../src/updater.m"
#include <assert.h>
@interface TestUpdateController : NSObject
@property(nonatomic) int checks;
@end
@implementation TestUpdateController
- (id)updater { return self; }
- (BOOL)canCheckForUpdates { return YES; }
- (void)checkForUpdates:(id)sender { self.checks++; }
@end
int main(void) {
    @autoreleasepool {
        assert(!alloy_updater_available());
        assert(!alloy_updater_check());
        TestUpdateController *mock = [TestUpdateController new];
        controller = (AlloySparkleControllerAPI *)mock;
        assert(alloy_updater_available());
        assert(alloy_updater_can_check());
        alloy_updater_set_blocked(1);
        assert(!alloy_updater_check());
        alloy_updater_set_blocked(0);
        assert(!alloy_updater_check()); // Test executable has no app feed URL.
        assert([feed_error_message(404, nil) containsString:@"尚未发布更新列表"]);
        assert(feed_error_message(200, nil) == nil);
        assert(feed_error_message(503, nil) != nil);
        checkingFeed = YES;
        assert(!alloy_updater_can_check());
        finish_feed_check(200, nil);
        assert(mock.checks == 1);
        alloy_updater_set_blocked(1);
        checkingFeed = YES;
        finish_feed_check(200, nil);
        assert(mock.checks == 1);
        assert(!checkingFeed);
        alloy_updater_set_blocked(0);
        AlloyUpdaterDelegate *delegate = [AlloyUpdaterDelegate new];
        __block BOOL installed = NO;
        alloy_updater_set_blocked(1);
        assert([delegate updater:nil shouldPostponeRelaunchForUpdate:nil untilInvokingBlock:^{ installed = YES; }]);
        assert(!installed);
        alloy_updater_set_blocked(0);
        [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.1]];
        assert(installed);
        assert(pendingInstall == nil);
        assert(![delegate updater:nil shouldPostponeRelaunchForUpdate:nil untilInvokingBlock:^{}]);
        controller = nil;
        puts("Native updater deferral and check tests passed");
    }
}
