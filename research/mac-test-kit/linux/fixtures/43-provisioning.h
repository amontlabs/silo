// A stand-in for the macOS 27 SDK's header, with the five options silovm sets (swift/Machine.swift).
// Not Apple's text: linux/test-43.sh reads it to check py/43-provisioning.py.
NS_ASSUME_NONNULL_BEGIN
VZ_EXPORT API_AVAILABLE(macos(27.0))
@interface VZGuestProvisioningOptions : NSObject <NSCopying>
- (BOOL)validateWithError:(NSError **)error;
@end

VZ_EXPORT API_AVAILABLE(macos(27.0))
@interface VZMacGuestProvisioningOptions : VZGuestProvisioningOptions
@property (copy) NSString *fullName;
@property (copy) NSString *username;
@property (copy) NSString *password;
@property BOOL logsInAutomatically;
@property BOOL enablesRemoteLogin;
@end
NS_ASSUME_NONNULL_END
