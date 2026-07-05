// Input validation functions

/// Validates username: 3-20 chars, alphanumeric + underscore only
pub fn validate_username(username: &str) -> Result<(), String> {
    let len = username.len();
    
    if len < 3 || len > 20 {
        return Err("Username must be 3-20 characters".to_string());
    }
    
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("Username can only contain letters, numbers, and underscore".to_string());
    }
    
    if username.contains(' ') {
        return Err("Username cannot contain spaces".to_string());
    }
    
    Ok(())
}

/// Validates name: 2-50 chars, spaces allowed
pub fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    let len = trimmed.len();
    
    if len < 2 || len > 50 {
        return Err("Name must be 2-50 characters".to_string());
    }
    
    Ok(())
}

/// Validates email: must contain @ with valid local and domain parts
pub fn validate_email(email: &str) -> Result<(), String> {
    let trimmed = email.trim();
    
    if trimmed.len() > 255 {
        return Err("Email must not exceed 255 characters".to_string());
    }
    
    // Count @ symbols
    let at_count = trimmed.matches('@').count();
    if at_count != 1 {
        return Err("Email must contain exactly one @ symbol".to_string());
    }
    
    // Split by @
    let parts: Vec<&str> = trimmed.split('@').collect();
    let local = parts[0];
    let domain = parts[1];
    
    // Check local part (before @)
    if local.is_empty() {
        return Err("Email local part cannot be empty".to_string());
    }
    
    // Check domain part (after @)
    if domain.is_empty() {
        return Err("Email domain cannot be empty".to_string());
    }
    
    if !domain.contains('.') {
        return Err("Email domain must contain a dot".to_string());
    }
    
    // Check that domain doesn't start or end with dot
    if domain.starts_with('.') || domain.ends_with('.') {
        return Err("Email domain cannot start or end with a dot".to_string());
    }
    
    Ok(())
}

/// Validates password: min 8 chars, recommended uppercase + digit
pub fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters".to_string());
    }
    
    // Check for recommended strength (not enforced, just warning)
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    
    if !has_uppercase || !has_digit {
        // Note: This is just a recommendation, not enforced
        // You could change this to return Err() to enforce it
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_username() {
        // Valid
        assert!(validate_username("abc").is_ok());
        assert!(validate_username("user_123").is_ok());
        assert!(validate_username("test_user_name_123").is_ok());
        
        // Too short
        assert!(validate_username("ab").is_err());
        
        // Too long
        assert!(validate_username("this_is_a_very_long_username").is_err());
        
        // Invalid characters
        assert!(validate_username("user name").is_err());
        assert!(validate_username("user-name").is_err());
        assert!(validate_username("user@name").is_err());
    }

    #[test]
    fn test_validate_name() {
        // Valid
        assert!(validate_name("Jo").is_ok());
        assert!(validate_name("John Doe").is_ok());
        assert!(validate_name("Mary Jane Watson").is_ok());
        
        // Too short
        assert!(validate_name("A").is_err());
        
        // Too long
        assert!(validate_name(&"a".repeat(51)).is_err());
    }

    #[test]
    fn test_validate_email() {
        // Valid
        assert!(validate_email("user@example.com").is_ok());
        assert!(validate_email("test.user@domain.co.uk").is_ok());
        
        // No @
        assert!(validate_email("userexample.com").is_err());
        
        // Multiple @
        assert!(validate_email("user@@example.com").is_err());
        
        // Empty local
        assert!(validate_email("@example.com").is_err());
        
        // Empty domain
        assert!(validate_email("user@").is_err());
        
        // No dot in domain
        assert!(validate_email("user@example").is_err());
        
        // Domain starts with dot
        assert!(validate_email("user@.example.com").is_err());
        
        // Too long
        assert!(validate_email(&format!("{}@example.com", "a".repeat(250))).is_err());
    }

    #[test]
    fn test_validate_password() {
        // Valid
        assert!(validate_password("Password1").is_ok());
        assert!(validate_password("mysecretpass123").is_ok());
        assert!(validate_password("12345678").is_ok()); // Meets minimum
        
        // Too short
        assert!(validate_password("Pass1").is_err());
        assert!(validate_password("1234567").is_err());
    }
}
